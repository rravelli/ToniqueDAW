//! Click track that follows the tempo map.

use std::sync::Arc;

use crate::graph::{Node, NodeIdentity, NodeProperties, ProcessContext, StateTransfer};
use crate::param::{AtomicParam, Smoother};
use crate::time::{BeatPos, SamplePos, TempoMap};

const TICK_HZ: f32 = 1000.0;
const ACCENT_HZ: f32 = 1800.0;
const ATTACK_S: f32 = 0.005;
const DECAY_PER_S: f32 = 80.0;
/// Past this the decay is inaudible (below -55 dB).
const CLICK_S: f32 = 0.08;

/// Square-wave click on every beat, higher-pitched on the first beat of a
/// bar. Silent while stopped; `gain` of 0 turns it off.
pub struct MetronomeNode {
    tempo: TempoMap,
    gain: Arc<AtomicParam>,
    gain_s: Smoother,
    /// Time since the current click started, in seconds; `None` when idle.
    click_t: Option<f32>,
    freq: f32,
    identity: Option<NodeIdentity>,
}

impl MetronomeNode {
    pub fn new(tempo: TempoMap, gain: Arc<AtomicParam>) -> Self {
        Self {
            tempo,
            gain_s: Smoother::new(gain.get()),
            gain,
            click_t: None,
            freq: TICK_HZ,
            identity: None,
        }
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }

    /// Next beat starting in `[start, end)`, as (offset in block, is accent).
    fn beat_in(&self, start: SamplePos, end: SamplePos, sr: f64) -> Option<(usize, bool)> {
        let first = self.tempo.samples_to_beats(start, sr).0.floor();
        let last = self.tempo.samples_to_beats(end, sr).0.ceil();
        let mut beat = first.max(0.0);
        // Compare in samples, so a beat is never missed or doubled because
        // of rounding at block boundaries.
        while beat <= last {
            let t = self.tempo.beats_to_samples(BeatPos(beat), sr);
            if (start..end).contains(&t) {
                let accent = beat % self.tempo.time_signature.beats_per_bar() == 0.0;
                return Some(((t - start) as usize, accent));
            }
            beat += 1.0;
        }
        None
    }
}

impl Node for MetronomeNode {
    fn properties(&self) -> NodeProperties {
        let p = NodeProperties::audio(2);
        match self.identity {
            Some(id) => p.with_identity(id),
            None => p,
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block: usize) {
        self.gain_s.set_sample_rate(sample_rate);
        self.gain_s.snap(&self.gain);
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        if !ctx.playing || ctx.jumped {
            self.click_t = None;
        }
        if !ctx.playing {
            return;
        }
        self.gain_s.retarget(&self.gain);
        let sr = ctx.sample_rate;
        let dt = 1.0 / sr as f32;
        let start = ctx.timeline_pos;
        // At most one beat per block unless tempos are absurd; blocks are
        // a few milliseconds long.
        let trigger = self.beat_in(start, start + ctx.block_len as SamplePos, sr);
        let (l, r) = ctx.audio_out.channel_pair_mut(0, 1);
        for i in 0..ctx.block_len {
            if let Some((at, accent)) = trigger
                && at == i
            {
                self.click_t = Some(0.0);
                self.freq = if accent { ACCENT_HZ } else { TICK_HZ };
            }
            let g = self.gain_s.next();
            let Some(t) = self.click_t else { continue };
            let env = if t < ATTACK_S {
                t / ATTACK_S
            } else {
                (-(t - ATTACK_S) * DECAY_PER_S).exp()
            };
            let square = if (t * self.freq).fract() < 0.5 {
                1.0
            } else {
                -1.0
            };
            let s = square * env * g;
            l[i] = s;
            r[i] = s;
            self.click_t = (t + dt < CLICK_S).then_some(t + dt);
        }
    }

    /// Keep the fresh instance (the tempo map may have changed) but let a
    /// click that's already sounding ring out.
    fn take_state_from(&mut self, previous: &mut dyn Node) -> StateTransfer {
        if let Some(prev) = previous.as_any_mut().and_then(|a| a.downcast_mut::<Self>()) {
            self.click_t = prev.click_t;
            self.freq = prev.freq;
            self.gain_s = prev.gain_s.clone();
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

    const SR: f64 = 48000.0;

    #[test]
    fn beats_land_on_exact_samples() {
        let node = MetronomeNode::new(TempoMap::new(120.0), Arc::new(AtomicParam::new(1.0)));
        // 120 bpm at 48 kHz: one beat every 24000 samples, accent every 4.
        assert_eq!(node.beat_in(0, 256, SR), Some((0, true)));
        assert_eq!(node.beat_in(23900, 24156, SR), Some((100, false)));
        assert_eq!(node.beat_in(96000, 96256, SR), Some((0, true)));
        assert_eq!(node.beat_in(24000 - 256, 24000, SR), None);
        assert_eq!(node.beat_in(1, 23999, SR), None);
    }
}
