//! Timeline playback: audio clips and MIDI clips.

use std::sync::Arc;

use crate::graph::{ContentId, Node, NodeIdentity, NodeProperties, ProcessContext, StateTransfer};
use crate::midi::{ActiveNotes, MidiMessage};
use crate::sample::SampleBuffer;
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
}

/// Reads a region of shared, pre-converted sample data at the transport
/// position. Position is derived from the timeline each block, so seeking
/// and looping need no state.
pub struct AudioClipNode {
    source: Arc<SampleBuffer>,
    place: ClipPlacement,
    gain: f32,
    identity: Option<NodeIdentity>,
}

impl AudioClipNode {
    pub fn new(source: Arc<SampleBuffer>, place: ClipPlacement, gain: f32) -> Self {
        Self { source, place, gain, identity: None }
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }

    #[inline]
    fn fade(&self, rel: SamplePos) -> f32 {
        let p = &self.place;
        let mut g = 1.0;
        if p.fade_in > 0 && rel < p.fade_in {
            g *= rel as f32 / p.fade_in as f32;
        }
        let to_end = p.length - rel;
        if p.fade_out > 0 && to_end < p.fade_out {
            g *= to_end as f32 / p.fade_out as f32;
        }
        g
    }
}

impl Node for AudioClipNode {
    fn properties(&self) -> NodeProperties {
        // Same source data + placement + gain => same output: dedupable.
        let content = ContentId::of(&(Arc::as_ptr(&self.source) as usize, self.place, self.gain.to_bits()));
        let p = NodeProperties::audio(2).with_content(content);
        match self.identity {
            Some(id) => p.with_identity(id),
            None => p,
        }
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        if !ctx.playing {
            return;
        }
        let p = self.place;
        let block_start = ctx.timeline_pos;
        let block_end = block_start + ctx.block_len as SamplePos;
        let a = block_start.max(p.start);
        let b = block_end.min(p.start + p.length);
        if a >= b {
            return;
        }
        let src_len = self.source.len() as SamplePos;
        let src_chans = self.source.num_channels();
        for ch in 0..2 {
            let src = self.source.channel(ch.min(src_chans - 1));
            let out = ctx.audio_out.channel_mut(ch);
            for t in a..b {
                let rel = t - p.start;
                let si = rel + p.source_offset;
                if si < 0 || si >= src_len {
                    continue;
                }
                out[(t - block_start) as usize] = src[si as usize] * self.gain * self.fade(rel);
            }
        }
    }
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
    pub fn new(notes: &[TimelineNote], channel: u8, clip_start: SamplePos, clip_end: SamplePos) -> Self {
        let mut events = Vec::with_capacity(notes.len() * 2);
        for n in notes {
            let on = n.start.max(clip_start);
            let off = (n.start + n.length).min(clip_end);
            if on >= off {
                continue;
            }
            events.push(TimedMidi { time: on, message: MidiMessage::NoteOn { channel, note: n.note, velocity: n.velocity } });
            events.push(TimedMidi { time: off, message: MidiMessage::NoteOff { channel, note: n.note, velocity: 0 } });
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
            self.active.release_all(ctx.midi_out, (self.clip_end - start) as u32);
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
