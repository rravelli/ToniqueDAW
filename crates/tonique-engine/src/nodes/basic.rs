use std::collections::VecDeque;
use std::f32::consts::TAU;
use std::sync::Arc;

use crate::audio::AudioBuffer;
use crate::graph::{ContentId, Node, NodeIdentity, NodeProperties, ProcessContext, StateTransfer};
use crate::meter::ChannelMeter;
use crate::midi::{MAX_MIDI_EVENTS_PER_BLOCK, MidiMessage};
use crate::param::{AtomicParam, Smoother};

/// Sums all inputs (audio and MIDI). Stateless, so it's content-addressed
/// and identical sums over identical inputs collapse into one.
pub struct SumNode {
    channels: usize,
}

impl SumNode {
    pub fn new(channels: usize) -> Self {
        Self { channels }
    }
}

impl Node for SumNode {
    fn properties(&self) -> NodeProperties {
        NodeProperties::audio(self.channels)
            .with_midi()
            .with_content(ContentId::of(&("sum", self.channels)))
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        ctx.sum_inputs_to_output();
        ctx.merge_midi_inputs_to_output();
    }
}

/// Pure delay line for audio and MIDI. Inserted by the compiler for latency
/// compensation, or used directly (with [`DelayNode::reporting`]) to model
/// a node with inherent latency.
pub struct DelayNode {
    delay: usize,
    channels: usize,
    has_midi: bool,
    report_latency: bool,
    ring: AudioBuffer,
    pos: usize,
    midi: VecDeque<(u64, MidiMessage)>,
    clock: u64,
}

impl DelayNode {
    /// A compensation delay (reports no latency of its own).
    pub fn new(delay: usize, channels: usize, has_midi: bool) -> Self {
        Self {
            delay,
            channels,
            has_midi,
            report_latency: false,
            ring: AudioBuffer::new(0, 0),
            pos: 0,
            midi: VecDeque::new(),
            clock: 0,
        }
    }

    /// A delay that reports its length as latency, like a lookahead plugin.
    pub fn reporting(delay: usize, channels: usize) -> Self {
        Self {
            report_latency: true,
            ..Self::new(delay, channels, false)
        }
    }
}

impl Node for DelayNode {
    fn properties(&self) -> NodeProperties {
        let mut p = NodeProperties::audio(self.channels);
        p.has_midi = self.has_midi;
        if self.report_latency {
            p.latency_samples = self.delay;
        }
        p
    }

    fn prepare(&mut self, _sample_rate: f64, _max_block: usize) {
        self.ring = AudioBuffer::new(self.channels, self.delay.max(1));
        self.midi = VecDeque::with_capacity(if self.has_midi {
            MAX_MIDI_EVENTS_PER_BLOCK * 4
        } else {
            0
        });
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        ctx.sum_inputs_to_output();
        let len = ctx.block_len;
        if self.delay > 0 {
            for ch in 0..self.channels {
                let ring = self.ring.channel_mut(ch);
                let mut p = self.pos;
                for s in ctx.audio_out.channel_mut(ch) {
                    std::mem::swap(&mut ring[p], s);
                    p = if p + 1 == self.delay { 0 } else { p + 1 };
                }
            }
            self.pos = (self.pos + len) % self.delay;
        }

        if self.has_midi {
            for i in 0..ctx.num_inputs() {
                for e in ctx.midi_input(i) {
                    // Bounded queue: drop rather than grow on the RT thread.
                    if self.midi.len() < self.midi.capacity() {
                        self.midi.push_back((
                            self.clock + e.offset as u64 + self.delay as u64,
                            e.message,
                        ));
                    }
                }
            }
            let end = self.clock + len as u64;
            // Events from several inputs may interleave; scan the whole
            // (small, bounded) queue rather than assuming it's sorted.
            let mut i = 0;
            while i < self.midi.len() {
                let (due, msg) = self.midi[i];
                if due < end {
                    ctx.midi_out
                        .push((due.saturating_sub(self.clock)) as u32, msg);
                    self.midi.remove(i);
                } else {
                    i += 1;
                }
            }
            ctx.midi_out.sort();
        }
        self.clock += len as u64;
    }
}

/// Stereo volume + equal-power pan, fed from shared atomic params. Used for
/// track and bus faders and for send levels.
pub struct VolumePanNode {
    volume: Arc<AtomicParam>,
    pan: Arc<AtomicParam>,
    /// Extra gain stage (mute/solo), ramped so toggling never clicks.
    gain: Option<Arc<AtomicParam>>,
    meter: Option<Arc<ChannelMeter>>,
    vol_s: Smoother,
    pan_s: Smoother,
    gain_s: Smoother,
    identity: Option<NodeIdentity>,
}

impl VolumePanNode {
    /// `volume` is linear gain, `pan` is -1 (left) ..= 1 (right).
    pub fn new(volume: Arc<AtomicParam>, pan: Arc<AtomicParam>) -> Self {
        Self {
            vol_s: Smoother::new(volume.get()),
            pan_s: Smoother::new(pan.get()),
            gain_s: Smoother::new(1.0),
            volume,
            pan,
            gain: None,
            meter: None,
            identity: None,
        }
    }

    pub fn with_gain(mut self, gain: Arc<AtomicParam>) -> Self {
        self.gain = Some(gain);
        self
    }

    /// Record the output (post-fader) levels into `meter`.
    pub fn with_meter(mut self, meter: Arc<ChannelMeter>) -> Self {
        self.meter = Some(meter);
        self
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }
}

impl Node for VolumePanNode {
    fn properties(&self) -> NodeProperties {
        let p = NodeProperties::audio(2).with_midi();
        match self.identity {
            Some(id) => p.with_identity(id),
            None => p,
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block: usize) {
        for (s, p) in [
            (&mut self.vol_s, &self.volume),
            (&mut self.pan_s, &self.pan),
        ] {
            s.set_sample_rate(sample_rate);
            s.snap(p);
        }
        self.gain_s.set_sample_rate(sample_rate);
        if let Some(g) = &self.gain {
            self.gain_s.snap(g);
        }
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        ctx.sum_inputs_to_output();
        ctx.merge_midi_inputs_to_output();
        self.vol_s.retarget(&self.volume);
        self.pan_s.retarget(&self.pan);
        if let Some(g) = &self.gain {
            self.gain_s.retarget(g);
        }
        let (l, r) = ctx.audio_out.channel_pair_mut(0, 1);
        for (l, r) in l.iter_mut().zip(r.iter_mut()) {
            let v = self.vol_s.next() * self.gain_s.next();
            let angle = (self.pan_s.next().clamp(-1.0, 1.0) + 1.0) * 0.25 * std::f32::consts::PI;
            // Equal-power law, normalised so centre is unity gain.
            let (gl, gr) = (
                angle.cos() * std::f32::consts::SQRT_2,
                angle.sin() * std::f32::consts::SQRT_2,
            );
            *l *= v * gl;
            *r *= v * gr;
        }
        if let Some(m) = &self.meter {
            m.record(&ctx.audio_out.as_block());
        }
    }

    /// Keep the fresh instance (params may have been re-bound) but continue
    /// the ramps from where the old one was, so rebuilds never jump.
    fn take_state_from(&mut self, previous: &mut dyn Node) -> StateTransfer {
        if let Some(prev) = previous.as_any_mut().and_then(|a| a.downcast_mut::<Self>()) {
            self.vol_s = prev.vol_s.clone();
            self.pan_s = prev.pan_s.clone();
            self.gain_s = prev.gain_s.clone();
        }
        StateTransfer::KeepNew
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Waveform {
    Sine,
    Saw,
}

/// Free-running test oscillator with atomic frequency and gain.
pub struct OscillatorNode {
    waveform: Waveform,
    freq: Arc<AtomicParam>,
    gain: Arc<AtomicParam>,
    freq_s: Smoother,
    gain_s: Smoother,
    phase: f32,
    sample_rate: f32,
}

impl OscillatorNode {
    pub fn new(waveform: Waveform, freq: Arc<AtomicParam>, gain: Arc<AtomicParam>) -> Self {
        Self {
            waveform,
            freq_s: Smoother::new(freq.get()),
            gain_s: Smoother::new(gain.get()),
            freq,
            gain,
            phase: 0.0,
            sample_rate: 48000.0,
        }
    }

    pub fn sine(freq: f32, gain: f32) -> Self {
        Self::new(
            Waveform::Sine,
            Arc::new(AtomicParam::new(freq)),
            Arc::new(AtomicParam::new(gain)),
        )
    }
}

impl Node for OscillatorNode {
    fn properties(&self) -> NodeProperties {
        NodeProperties::audio(1)
    }

    fn prepare(&mut self, sample_rate: f64, _max_block: usize) {
        self.sample_rate = sample_rate as f32;
        for (s, p) in [
            (&mut self.freq_s, &self.freq),
            (&mut self.gain_s, &self.gain),
        ] {
            s.set_sample_rate(sample_rate);
            s.snap(p);
        }
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        self.freq_s.retarget(&self.freq);
        self.gain_s.retarget(&self.gain);
        for s in ctx.audio_out.channel_mut(0) {
            let v = match self.waveform {
                Waveform::Sine => (self.phase * TAU).sin(),
                Waveform::Saw => 2.0 * self.phase - 1.0,
            };
            *s = v * self.gain_s.next();
            self.phase = (self.phase + self.freq_s.next() / self.sample_rate).fract();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{BlockInfo, CompileOptions, GraphDescription, compile};
    use crate::midi::MidiEventList;

    /// Emits one note-on at a fixed offset of the first block.
    struct OneNote {
        sent: bool,
    }
    impl Node for OneNote {
        fn properties(&self) -> NodeProperties {
            NodeProperties::midi()
        }
        fn process(&mut self, ctx: &mut ProcessContext) {
            if !std::mem::replace(&mut self.sent, true) {
                ctx.midi_out.push(
                    5,
                    MidiMessage::NoteOn {
                        channel: 0,
                        note: 60,
                        velocity: 100,
                    },
                );
            }
        }
    }

    struct Capture(Arc<std::sync::Mutex<Vec<(usize, u32)>>>, usize);
    impl Node for Capture {
        fn properties(&self) -> NodeProperties {
            NodeProperties::midi()
        }
        fn process(&mut self, ctx: &mut ProcessContext) {
            for e in ctx.midi_input(0) {
                self.0.lock().unwrap().push((self.1, e.offset));
            }
            self.1 += 1;
        }
    }

    #[test]
    fn delays_midi_across_blocks() {
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut d = GraphDescription::new();
        let src = d.add(OneNote { sent: false }, &[]);
        let del = d.add(DelayNode::new(10, 0, true), &[src]);
        let cap = d.add(Capture(log.clone(), 0), &[del]);
        d.set_output(cap);
        let mut g = compile(
            d,
            &CompileOptions {
                sample_rate: 48000.0,
                max_block: 8,
            },
        )
        .unwrap();
        let info = BlockInfo {
            block_len: 8,
            sample_rate: 48000.0,
            timeline_pos: 0,
            playing: true,
            jumped: false,
        };
        for _ in 0..4 {
            g.process_sequential(&info);
        }
        // offset 5 + 10 = 15 -> block 1, offset 7
        assert_eq!(*log.lock().unwrap(), vec![(1, 7)]);
        let _ = MidiEventList::default();
    }

    #[test]
    fn pan_is_equal_power_and_centre_is_unity() {
        let vol = Arc::new(AtomicParam::new(1.0));
        let pan = Arc::new(AtomicParam::new(0.0));
        let mut d = GraphDescription::new();
        let osc = d.add(
            OscillatorNode::new(
                Waveform::Saw,
                Arc::new(AtomicParam::new(0.0)),
                Arc::new(AtomicParam::new(1.0)),
            ),
            &[],
        );
        let vp = d.add(VolumePanNode::new(vol, pan.clone()), &[osc]);
        d.set_output(vp);
        let mut g = compile(
            d,
            &CompileOptions {
                sample_rate: 48000.0,
                max_block: 4,
            },
        )
        .unwrap();
        let info = BlockInfo {
            block_len: 4,
            sample_rate: 48000.0,
            timeline_pos: 0,
            playing: true,
            jumped: false,
        };
        g.process_sequential(&info);
        // saw at 0 Hz, phase 0 -> constant -1
        assert!((g.output(4).channel(0)[3] + 1.0).abs() < 1e-6);
        pan.set(1.0, 0.0);
        g.process_sequential(&info);
        let out = g.output(4);
        assert!(out.channel(0)[3].abs() < 1e-6);
        assert!((out.channel(1)[3] + std::f32::consts::SQRT_2).abs() < 1e-5);
    }
}
