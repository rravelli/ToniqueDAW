//! Timeline playback: audio clips and MIDI clips.

use std::sync::Arc;

use crate::graph::{ContentId, Node, NodeIdentity, NodeProperties, ProcessContext, StateTransfer};
use crate::midi::{ActiveNotes, MidiMessage};
use crate::param::{AtomicParam, Smoother};
use crate::sample::SampleBuffer;
use crate::stretch::{StretchSettings, Stretcher};
use crate::time::SamplePos;

/// Placement of an audio clip on the timeline, in engine samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClipPlacement {
    pub start: SamplePos,
    pub length: SamplePos,
    /// Where in the source the clip starts reading.
    pub source_offset: SamplePos,
    pub fade_in: SamplePos,
    pub fade_out: SamplePos,
    /// Reading reaches the loop's end, then goes on from its start.
    pub looped: Option<LoopPlacement>,
}

/// A clip's loop, in source samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LoopPlacement {
    pub start: SamplePos,
    pub end: SamplePos,
    /// How long before the end the source starts crossfading into what's
    /// just before the loop's start, so the seam is continuous.
    pub crossfade: SamplePos,
}

impl LoopPlacement {
    /// The loop over `start..end`, crossfading over up to `crossfade` (less
    /// on short loops, or with little before the start).
    pub fn new(start: SamplePos, end: SamplePos, crossfade: SamplePos) -> Self {
        let crossfade = crossfade.min((end - start) / 2).min(start).max(0);
        Self {
            start,
            end,
            crossfade,
        }
    }

    /// Where `at` (in the source, going on past the end) reads, and when
    /// crossfading, from where too and how much of it (0 to 1).
    #[inline]
    fn read(&self, at: SamplePos) -> (SamplePos, Option<(SamplePos, f32)>) {
        let len = self.end - self.start;
        let at = if at >= self.end && len > 0 {
            self.start + (at - self.start).rem_euclid(len)
        } else {
            at
        };
        let into = at - (self.end - self.crossfade);
        if self.crossfade > 0 && into >= 0 && at < self.end {
            let t = (into as f32 + 0.5) / self.crossfade as f32;
            return (at, Some((at - len, t)));
        }
        (at, None)
    }
}

/// How a clip plays its source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClipWarp {
    /// As it is: a source sample per output sample.
    None,
    /// Resampled, as tape: `speed` source samples per output sample, the
    /// pitch moving with it.
    Repitch { speed: f64 },
    /// Stretched: `speed` source samples per output sample, the pitch kept,
    /// and transposed by `semitones` (by the clip's [`Stretcher`], as
    /// `settings` say).
    Stretch {
        speed: f64,
        semitones: f32,
        settings: StretchSettings,
    },
}

impl ClipWarp {
    /// Source samples per output sample.
    pub fn speed(&self) -> f64 {
        match *self {
            Self::None => 1.0,
            Self::Repitch { speed } | Self::Stretch { speed, .. } => speed,
        }
    }

    /// For identifying content: its numbers' bits.
    fn bits(&self) -> (u8, u64, u32, StretchSettings) {
        match *self {
            Self::None => (0, 0, 0, StretchSettings::default()),
            Self::Repitch { speed } => (1, speed.to_bits(), 0, StretchSettings::default()),
            Self::Stretch {
                speed,
                semitones,
                settings,
            } => (2, speed.to_bits(), semitones.to_bits(), settings),
        }
    }

    /// Stretched, its settings.
    fn settings(&self) -> Option<StretchSettings> {
        match *self {
            Self::Stretch { settings, .. } => Some(settings),
            _ => None,
        }
    }
}

/// Where a stretched clip's [`Stretcher`] is in its source.
struct Stretching {
    stretcher: Box<dyn Stretcher>,
    /// Next source position fed (going on past a loop's end, see
    /// [`LoopPlacement::read`]), with its fraction: ahead of what's heard
    /// by the stretcher's latency. NaN before the first seek.
    fed: f64,
    /// The speed last played at: the output queued in the stretcher was
    /// made at it.
    speed: f64,
    /// The timeline position the next block starts at if playback goes on
    /// without a jump; none when stopped or never played.
    next: Option<SamplePos>,
    /// Interleaved stereo, sized in `prepare` so processing never
    /// allocates.
    input: Vec<f32>,
    output: Vec<f32>,
}

/// Reads a region of shared, pre-converted sample data at the transport
/// position. As it is or resampled, the position is derived from the
/// timeline each block, so seeking and looping need no state; stretched,
/// its stretcher is fed ahead of the transport, and starts over (fed again
/// from the right place) at each jump.
pub struct AudioClipNode {
    source: Arc<SampleBuffer>,
    place: ClipPlacement,
    warp: ClipWarp,
    /// For [`ClipWarp::Stretch`] (resampled without one).
    stretching: Option<Stretching>,
    /// Linear, shared with the clip's parameter: changes are heard at once.
    gain: Arc<AtomicParam>,
    gain_s: Smoother,
    /// Balance, -1 to 1, shared with the clip's parameter too.
    pan: Arc<AtomicParam>,
    pan_s: Smoother,
    identity: Option<NodeIdentity>,
}

impl AudioClipNode {
    pub fn new(
        source: Arc<SampleBuffer>,
        place: ClipPlacement,
        gain: Arc<AtomicParam>,
        pan: Arc<AtomicParam>,
    ) -> Self {
        Self {
            source,
            place,
            warp: ClipWarp::None,
            stretching: None,
            gain_s: Smoother::new(gain.get()),
            gain,
            pan_s: Smoother::new(pan.get()),
            pan,
            identity: None,
        }
    }

    /// Play the source `warp`ed. Stretched, with `stretcher` (built off the
    /// audio thread), or resampled without one.
    pub fn with_warp(mut self, warp: ClipWarp, stretcher: Option<Box<dyn Stretcher>>) -> Self {
        self.warp = warp;
        self.stretching = None;
        if let ClipWarp::Stretch {
            speed,
            semitones,
            settings,
        } = warp
        {
            match stretcher {
                Some(mut stretcher) => {
                    stretcher.set_transpose(semitones, settings.formants);
                    self.stretching = Some(Stretching {
                        stretcher,
                        fed: f64::NAN,
                        speed,
                        next: None,
                        input: Vec::new(),
                        output: Vec::new(),
                    });
                }
                None => self.warp = ClipWarp::Repitch { speed },
            }
        }
        self
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }
}

impl ClipPlacement {
    /// The fades' gain at `rel` from the clip's start.
    #[inline]
    fn fade(&self, rel: SamplePos) -> f32 {
        let mut g = 1.0;
        if self.fade_in > 0 && rel < self.fade_in {
            g *= rel as f32 / self.fade_in as f32;
        }
        let to_end = self.length - rel;
        if self.fade_out > 0 && to_end < self.fade_out {
            g *= to_end as f32 / self.fade_out as f32;
        }
        g
    }
}

/// A source's channels and loop, looked up once a block rather than at
/// each frame.
#[derive(Clone, Copy)]
struct SourceView<'a> {
    left: &'a [f32],
    right: &'a [f32],
    len: SamplePos,
    looped: Option<LoopPlacement>,
}

impl<'a> SourceView<'a> {
    fn new(source: &'a SampleBuffer, looped: Option<LoopPlacement>) -> Self {
        Self {
            left: source.channel(0),
            right: source.channel(1.min(source.num_channels() - 1)),
            len: source.len() as SamplePos,
            looped,
        }
    }

    /// The frame at `at` (going on past a loop's end, crossfaded at its
    /// seam); silence outside the source.
    #[inline]
    fn frame(&self, at: SamplePos) -> (f32, f32) {
        let (si, blend) = match self.looped {
            Some(looped) => looped.read(at),
            None => (at, None),
        };
        if !(0..self.len).contains(&si) {
            return (0.0, 0.0);
        }
        let (mut l, mut r) = (self.left[si as usize], self.right[si as usize]);
        if let Some((from, mix)) = blend
            && (0..self.len).contains(&from)
        {
            let from = from as usize;
            l += (self.left[from] - l) * mix;
            r += (self.right[from] - r) * mix;
        }
        (l, r)
    }

    /// The frame at `at` and a fraction, between frames: a cubic through
    /// the four around it.
    #[inline]
    fn frame_between(&self, at: f64) -> (f32, f32) {
        let i = at.floor();
        let f = (at - i) as f32;
        let i = i as SamplePos;
        let [a, b, c, d] = [i - 1, i, i + 1, i + 2].map(|j| self.frame(j));
        (
            hermite(a.0, b.0, c.0, d.0, f),
            hermite(a.1, b.1, c.1, d.1, f),
        )
    }
}

/// Catmull-Rom between `b` and `c` (at `t` from 0 to 1), `a` before and `d`
/// after.
#[inline]
fn hermite(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    let c1 = 0.5 * (c - a);
    let c2 = a - 2.5 * b + 2.0 * c - 0.5 * d;
    let c3 = 0.5 * (d - a) + 1.5 * (b - c);
    ((c3 * t + c2) * t + c1) * t + b
}

/// Interleave `frames` frames of `source` from `from` (with its loop) into
/// `out`.
fn fill(
    out: &mut [f32],
    source: &SampleBuffer,
    looped: Option<LoopPlacement>,
    from: SamplePos,
    frames: usize,
) {
    let source = SourceView::new(source, looped);
    for (k, pair) in out[..frames * 2]
        .as_chunks_mut::<2>()
        .0
        .iter_mut()
        .enumerate()
    {
        let (l, r) = source.frame(from + k as SamplePos);
        *pair = [l, r];
    }
}

/// Where a clip node writes a block: through its gain, pan and fades.
struct Output<'a> {
    left: &'a mut [f32],
    right: &'a mut [f32],
    block_start: SamplePos,
    place: &'a ClipPlacement,
    gain_s: &'a mut Smoother,
    pan_s: &'a mut Smoother,
}

impl Output<'_> {
    /// Write the clip from `a` to `b` (timeline positions in the block),
    /// each frame as `frame` gives it.
    #[inline(always)]
    fn write(&mut self, a: SamplePos, b: SamplePos, frame: impl Fn(SamplePos) -> (f32, f32)) {
        for t in a..b {
            let gain = self.gain_s.next();
            let (gain_l, gain_r) = balance(self.pan_s.next());
            let (l, r) = frame(t);
            let g = gain * self.place.fade(t - self.place.start);
            let at = (t - self.block_start) as usize;
            self.left[at] = l * g * gain_l;
            self.right[at] = r * g * gain_r;
        }
    }
}

impl Stretching {
    /// Start over so the next output frame is the source at `at`: fed from
    /// there, the output lagging behind it played out and dropped.
    fn seek(&mut self, source: &SampleBuffer, looped: Option<LoopPlacement>, at: f64, speed: f64) {
        let start = at.round() as SamplePos;
        let ahead = self.stretcher.input_latency();
        fill(&mut self.input, source, looped, start, ahead);
        self.stretcher.seek(&self.input[..ahead * 2], speed);
        // The processing time is now at `start`; output comes its latency
        // behind it: play that out and drop it.
        let behind = self.stretcher.output_latency();
        let fed = (start + ahead as SamplePos) as f64;
        let to = fed + behind as f64 * speed;
        let frames = (to.floor() - fed.floor()) as usize;
        fill(&mut self.input, source, looped, fed as SamplePos, frames);
        self.stretcher
            .process(&self.input[..frames * 2], &mut self.output[..behind * 2]);
        self.fed = to;
        self.speed = speed;
    }

    /// The source position of the next output frame: what's fed, less
    /// what's in the stretcher, its output queued at the speed it was made.
    fn heard(&self) -> f64 {
        let (ahead, behind) = (
            self.stretcher.input_latency(),
            self.stretcher.output_latency(),
        );
        self.fed - ahead as f64 - behind as f64 * self.speed
    }

    /// Whether playing on from where it is gives the source at `at` (as
    /// after a graph rebuilt for a tempo change: the same beat, at another
    /// speed), rather than seeking, which starts it over. Going on at
    /// another speed, the feed moves by the difference over its queued
    /// output so the source stays on the timeline once that's played out.
    fn goes_on(&mut self, at: f64, speed: f64) -> bool {
        // Within a timeline sample, as positions are rounded to samples
        // (never before a first seek: `fed` is NaN).
        let off = (self.heard() - at).abs();
        if off.is_nan() || off > 1.0 + speed {
            return false;
        }
        let behind = self.stretcher.output_latency() as f64;
        self.fed += behind * (speed - self.speed);
        self.speed = speed;
        true
    }

    /// The next `frames` frames of output, into `self.output`.
    fn play(
        &mut self,
        source: &SampleBuffer,
        looped: Option<LoopPlacement>,
        frames: usize,
        speed: f64,
    ) {
        let to = self.fed + frames as f64 * speed;
        let feed = (to.floor() - self.fed.floor()) as usize;
        fill(
            &mut self.input,
            source,
            looped,
            self.fed.floor() as SamplePos,
            feed,
        );
        self.stretcher
            .process(&self.input[..feed * 2], &mut self.output[..frames * 2]);
        self.fed = to;
        self.speed = speed;
    }
}

impl Node for AudioClipNode {
    fn properties(&self) -> NodeProperties {
        // Same source data + placement + warp + gain => same output:
        // dedupable. The gain and pan change live, so they're the same only
        // if they're the same parameters.
        let content = ContentId::of(&(
            Arc::as_ptr(&self.source) as usize,
            self.place,
            self.warp.bits(),
            Arc::as_ptr(&self.gain) as usize,
            Arc::as_ptr(&self.pan) as usize,
        ));
        let p = NodeProperties::audio(2).with_content(content);
        match self.identity {
            Some(id) => p.with_identity(id),
            None => p,
        }
    }

    fn prepare(&mut self, sample_rate: f64, max_block: usize) {
        for (s, p) in [(&mut self.gain_s, &self.gain), (&mut self.pan_s, &self.pan)] {
            s.set_sample_rate(sample_rate);
            s.snap(p);
        }
        if let Some(st) = &mut self.stretching {
            // The most fed or played at once: a block, or a seek.
            let (ahead, behind) = (st.stretcher.input_latency(), st.stretcher.output_latency());
            let speed = self.warp.speed();
            let fed = ((max_block.max(behind) as f64 * speed).ceil() as usize + 2).max(ahead);
            st.input.resize(fed * 2, 0.0);
            st.output.resize(max_block.max(behind) * 2, 0.0);
        }
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        let p = self.place;
        let block_start = ctx.timeline_pos;
        let block_end = block_start + ctx.block_len as SamplePos;
        let a = block_start.max(p.start);
        let b = block_end.min(p.start + p.length);
        // Silent: changes take effect at once, so none is ramped in when
        // the clip next plays.
        if !ctx.playing || a >= b {
            self.gain_s.snap(&self.gain);
            self.pan_s.snap(&self.pan);
            if let Some(st) = &mut self.stretching {
                st.next = None;
            }
            return;
        }
        self.gain_s.retarget(&self.gain);
        self.pan_s.retarget(&self.pan);

        // Stretched: played ahead into the stretcher's output, from the
        // right place again after a jump.
        let speed = self.warp.speed();
        if let Some(st) = &mut self.stretching {
            if ctx.jumped || st.next != Some(a) {
                let at = p.source_offset as f64 + (a - p.start) as f64 * speed;
                if ctx.jumped || !st.goes_on(at, speed) {
                    st.seek(&self.source, p.looped, at, speed);
                }
            }
            st.play(&self.source, p.looped, (b - a) as usize, speed);
            st.next = Some(b);
        }

        // One loop per way of playing, so none decides it at each frame.
        let (left, right) = ctx.audio_out.channel_pair_mut(0, 1);
        let mut out = Output {
            left,
            right,
            block_start,
            place: &p,
            gain_s: &mut self.gain_s,
            pan_s: &mut self.pan_s,
        };
        let source = SourceView::new(&self.source, p.looped);
        let offset = p.source_offset;
        match (&self.stretching, self.warp) {
            (Some(st), _) => out.write(a, b, |t| {
                let k = (t - a) as usize * 2;
                (st.output[k], st.output[k + 1])
            }),
            (None, ClipWarp::None) => out.write(a, b, |t| source.frame(t - p.start + offset)),
            (None, _) => out.write(a, b, |t| {
                source.frame_between(offset as f64 + (t - p.start) as f64 * speed)
            }),
        }
    }

    /// A stretched clip keeps its stretcher, fed so far, when the graph is
    /// rebuilt (its gain, another clip... changed): going on where it was
    /// if it plays the same source the same way, from the right place
    /// otherwise. With another window, it has a new stretcher, which starts
    /// over.
    fn take_state_from(&mut self, previous: &mut dyn Node) -> StateTransfer {
        let Some(prev) = previous.as_any_mut().and_then(|a| a.downcast_mut::<Self>()) else {
            return StateTransfer::KeepNew;
        };
        let same_audio = Arc::ptr_eq(&self.source, &prev.source) && self.place == prev.place;
        let same_speed = self.warp.speed() == prev.warp.speed();
        let same_window =
            self.warp.settings().map(|s| s.window) == prev.warp.settings().map(|s| s.window);
        if let (Some(new), Some(old)) = (&mut self.stretching, &mut prev.stretching)
            && same_window
        {
            std::mem::swap(&mut new.stretcher, &mut old.stretcher);
            if let ClipWarp::Stretch {
                semitones,
                settings,
                ..
            } = self.warp
            {
                new.stretcher.set_transpose(semitones, settings.formants);
            }
            if same_audio && same_speed && new.input.len() == old.input.len() {
                std::mem::swap(&mut new.input, &mut old.input);
                std::mem::swap(&mut new.output, &mut old.output);
                new.next = old.next;
            }
            // Where it is in the same source, read the same way: placed
            // otherwise (the tempo changed...), it goes on from there if
            // that's where it plays from (see `Stretching::goes_on`).
            if Arc::ptr_eq(&self.source, &prev.source) && self.place.looped == prev.place.looped {
                new.fed = old.fed;
                new.speed = old.speed;
            }
        }
        StateTransfer::KeepNew
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

/// Left and right gains of a balance from -1 (left only) to 1 (right
/// only): the side turned away from is lowered, the other stays as is, so
/// the centre is unity and a stereo image stays whole.
#[inline]
pub fn balance(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(-1.0, 1.0);
    ((1.0 - pan).min(1.0), (1.0 + pan).min(1.0))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimedMidi {
    pub time: SamplePos,
    pub message: MidiMessage,
}

/// A note placed on the timeline, in engine samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimelineNote {
    pub start: SamplePos,
    pub length: SamplePos,
    pub note: u8,
    pub velocity: u8,
}

/// Emits a MIDI sequence at the transport position. Releases held notes on
/// stop, seek/loop and at the clip end, so nothing ever hangs.
pub struct MidiClipNode {
    events: Arc<[TimedMidi]>,
    clip_start: SamplePos,
    clip_end: SamplePos,
    active: ActiveNotes,
    /// Notes inherited from a replaced instance, released on the next block.
    inherited: ActiveNotes,
    identity: Option<NodeIdentity>,
}

impl MidiClipNode {
    /// Notes are clipped to `[clip_start, clip_end)`.
    pub fn new(
        notes: &[TimelineNote],
        channel: u8,
        clip_start: SamplePos,
        clip_end: SamplePos,
    ) -> Self {
        let mut events = Vec::with_capacity(notes.len() * 2);
        for n in notes {
            let on = n.start.max(clip_start);
            let off = (n.start + n.length).min(clip_end);
            if on >= off {
                continue;
            }
            events.push(TimedMidi {
                time: on,
                message: MidiMessage::NoteOn {
                    channel,
                    note: n.note,
                    velocity: n.velocity,
                },
            });
            events.push(TimedMidi {
                time: off,
                message: MidiMessage::NoteOff {
                    channel,
                    note: n.note,
                    velocity: 0,
                },
            });
        }
        // Note-offs sort before note-ons at the same time so a repeated
        // note retriggers instead of being cut.
        events.sort_by_key(|e| (e.time, matches!(e.message, MidiMessage::NoteOn { .. })));
        Self {
            events: events.into(),
            clip_start,
            clip_end,
            active: ActiveNotes::default(),
            inherited: ActiveNotes::default(),
            identity: None,
        }
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }
}

impl Node for MidiClipNode {
    fn properties(&self) -> NodeProperties {
        let p = NodeProperties::midi();
        match self.identity {
            Some(id) => p.with_identity(id),
            None => p,
        }
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        // Notes started by the instance we replaced (the clip was edited):
        // the new event list may not contain their note-offs.
        self.inherited.release_all(ctx.midi_out, 0);
        if !ctx.playing || ctx.jumped {
            self.active.release_all(ctx.midi_out, 0);
            if !ctx.playing {
                return;
            }
        }
        let start = ctx.timeline_pos;
        let end = start + ctx.block_len as SamplePos;
        if end <= self.clip_start || start >= self.clip_end {
            return;
        }
        let first = self.events.partition_point(|e| e.time < start);
        for e in self.events[first..].iter().take_while(|e| e.time < end) {
            if ctx.midi_out.push((e.time - start) as u32, e.message) {
                self.active.observe(&e.message);
            }
        }
        if (start..end).contains(&self.clip_end) {
            self.active
                .release_all(ctx.midi_out, (self.clip_end - start) as u32);
        }
    }

    fn take_state_from(&mut self, previous: &mut dyn Node) -> StateTransfer {
        if let Some(prev) = previous.as_any_mut().and_then(|a| a.downcast_mut::<Self>()) {
            self.inherited = std::mem::take(&mut prev.active);
        }
        StateTransfer::KeepNew
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{BlockInfo, CompileOptions, CompiledGraph, GraphDescription, compile};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const OPTS: CompileOptions = CompileOptions {
        sample_rate: 48000.0,
        max_block: 256,
    };

    /// Gives silence, counting the times it's started over.
    struct Counting(Arc<AtomicUsize>);

    impl Stretcher for Counting {
        fn input_latency(&self) -> usize {
            64
        }
        fn output_latency(&self) -> usize {
            64
        }
        fn set_transpose(&mut self, _: f32, _: bool) {}
        fn seek(&mut self, _: &[f32], _: f64) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
        fn process(&mut self, _: &[f32], output: &mut [f32]) {
            output.fill(0.0);
        }
    }

    /// A graph of a clip of 4 s of source from `start`, stretched at
    /// `speed`.
    fn graph(
        source: &Arc<SampleBuffer>,
        start: SamplePos,
        speed: f64,
        seeks: &Arc<AtomicUsize>,
    ) -> CompiledGraph {
        let place = ClipPlacement {
            start,
            length: (4.0 * 48000.0 / speed) as SamplePos,
            source_offset: 0,
            fade_in: 0,
            fade_out: 0,
            looped: None,
        };
        let node = AudioClipNode::new(
            source.clone(),
            place,
            Arc::new(AtomicParam::new(1.0)),
            Arc::new(AtomicParam::new(0.0)),
        )
        .with_warp(
            ClipWarp::Stretch {
                speed,
                semitones: 0.0,
                settings: StretchSettings::default(),
            },
            Some(Box::new(Counting(seeks.clone()))),
        )
        .with_identity(NodeIdentity::of(&"clip"));
        let mut d = GraphDescription::new();
        let n = d.add(node, &[]);
        d.set_output(n);
        compile(d, &OPTS).unwrap()
    }

    fn block(g: &mut CompiledGraph, at: SamplePos, jumped: bool) {
        g.process_sequential(&BlockInfo {
            block_len: 256,
            sample_rate: 48000.0,
            timeline_pos: at,
            playing: true,
            jumped,
        });
    }

    /// Rebuilt for a tempo change, a stretched clip plays on where it was in
    /// its source, at its new speed: its stretcher isn't started over. Moved
    /// elsewhere in its source, it is.
    #[test]
    fn stretchers_go_on_through_tempo_changes() {
        let source = Arc::new(SampleBuffer::new(vec![vec![0.0; 48000 * 4]], 48000.0));
        let seeks = Arc::new(AtomicUsize::new(0));
        // At half speed, from the timeline's start: 4 blocks in, 512 source
        // frames are played.
        let mut old = graph(&source, 0, 0.5, &seeks);
        block(&mut old, 0, true);
        for i in 1..4 {
            block(&mut old, i * 256, false);
        }
        assert_eq!(seeks.load(Ordering::Relaxed), 1, "once, from the start");

        // Half the tempo: a quarter of the speed, the same place in the
        // source twice as far into the timeline.
        let mut new = graph(&source, 0, 0.25, &seeks);
        new.adopt_state_from(&mut old);
        block(&mut new, 2048, false);
        block(&mut new, 2048 + 256, false);
        assert_eq!(seeks.load(Ordering::Relaxed), 1, "played on");

        // The clip moved: elsewhere in its source, it starts over.
        let mut moved = graph(&source, 1000, 0.25, &seeks);
        moved.adopt_state_from(&mut new);
        block(&mut moved, 2048 + 512, false);
        assert_eq!(seeks.load(Ordering::Relaxed), 2, "started over");
    }
}
