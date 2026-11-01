//! Saturation: drive into a waveshaper, run at 4× the sample rate so the
//! harmonics it adds don't fold back as aliasing.

use std::sync::Arc;

use crate::dsp::{Oversampler, db_to_gain};
use crate::graph::{Node, NodeIdentity, NodeProperties, ProcessContext, StateTransfer};
use crate::param::{AtomicParam, Smoother};

/// How a [`SaturationNode`] bends its input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SaturationCurve {
    /// Rounds peaks off smoothly (tanh).
    SoftClip,
    /// Softly and unevenly: the halves of the wave bend differently,
    /// adding even harmonics.
    Tape,
    /// Cuts peaks flat at full scale.
    HardClip,
}

/// How much the tape curve leans to a side.
const TAPE_BIAS: f32 = 0.2;

impl SaturationCurve {
    pub const ALL: [SaturationCurve; 3] = [
        SaturationCurve::SoftClip,
        SaturationCurve::Tape,
        SaturationCurve::HardClip,
    ];

    /// The curve at `x`. Each rises with a slope of 1 through 0, so quiet
    /// audio comes out as loud as it went in.
    #[inline]
    pub fn shape(self, x: f32) -> f32 {
        match self {
            SaturationCurve::SoftClip => x.tanh(),
            SaturationCurve::Tape => {
                let bias = TAPE_BIAS.tanh();
                ((x + TAPE_BIAS).tanh() - bias) / (1.0 - bias * bias)
            }
            SaturationCurve::HardClip => x.clamp(-1.0, 1.0),
        }
    }
}

/// Stereo saturation: drive (dB) into the curve, output gain (dB), then a
/// dry/wet mix (0 to 1). The oversampler delays the audio by
/// [`Oversampler::LATENCY`] samples, reported as latency; the dry signal
/// is delayed as much so they mix in step.
pub struct SaturationNode {
    curve: SaturationCurve,
    params: [Arc<AtomicParam>; 3],
    /// Drive, output, mix.
    smoothers: [Smoother; 3],
    oversamplers: [Oversampler; 2],
    dry: [[f32; Oversampler::LATENCY]; 2],
    dry_pos: usize,
    /// Removes the offset the tape curve leaves: input and output memory.
    dc: [(f32, f32); 2],
    dc_coef: f32,
    identity: Option<NodeIdentity>,
}

impl SaturationNode {
    pub fn new(curve: SaturationCurve, [drive, output, mix]: [Arc<AtomicParam>; 3]) -> Self {
        let params = [drive, output, mix];
        Self {
            curve,
            smoothers: std::array::from_fn(|i| Smoother::new(params[i].get())),
            params,
            oversamplers: Default::default(),
            dry: [[0.0; Oversampler::LATENCY]; 2],
            dry_pos: 0,
            dc: [(0.0, 0.0); 2],
            dc_coef: 0.999,
            identity: None,
        }
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }
}

impl Node for SaturationNode {
    fn properties(&self) -> NodeProperties {
        let p = NodeProperties::audio(2).with_latency(Oversampler::LATENCY);
        match self.identity {
            Some(id) => p.with_identity(id),
            None => p,
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block: usize) {
        // A high-pass around 10 Hz.
        self.dc_coef = (-2.0 * std::f64::consts::PI * 10.0 / sample_rate).exp() as f32;
        for (s, p) in self.smoothers.iter_mut().zip(&self.params) {
            s.set_sample_rate(sample_rate);
            s.snap(p);
        }
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        ctx.sum_inputs_to_output();
        for (s, p) in self.smoothers.iter_mut().zip(&self.params) {
            s.retarget(p);
        }
        let curve = self.curve;
        let (l, r) = ctx.audio_out.channel_pair_mut(0, 1);
        for (l, r) in l.iter_mut().zip(r.iter_mut()) {
            let [drive, output, mix] = self.smoothers.each_mut().map(|s| s.next());
            let (drive, output) = (db_to_gain(drive), db_to_gain(output));
            let mix = mix.clamp(0.0, 1.0);
            let p = self.dry_pos;
            for (ch, s) in [l, r].into_iter().enumerate() {
                let wet = self.oversamplers[ch].process(*s, |x| curve.shape(x * drive));
                let (x1, y1) = &mut self.dc[ch];
                let y = wet - *x1 + self.dc_coef * *y1;
                *x1 = wet;
                *y1 = if y.abs() < 1e-20 { 0.0 } else { y };
                let dry = std::mem::replace(&mut self.dry[ch][p], *s);
                *s = dry * (1.0 - mix) + y * output * mix;
            }
            self.dry_pos = (p + 1) % Oversampler::LATENCY;
        }
    }

    fn tail_samples(&self) -> usize {
        Oversampler::LATENCY
    }

    fn take_state_from(&mut self, previous: &mut dyn Node) -> StateTransfer {
        if let Some(prev) = previous.as_any_mut().and_then(|a| a.downcast_mut::<Self>()) {
            // The curve may have changed: the audio in flight stays.
            std::mem::swap(&mut self.oversamplers, &mut prev.oversamplers);
            self.dry = prev.dry;
            self.dry_pos = prev.dry_pos;
            self.dc = prev.dc;
            self.smoothers = prev.smoothers.clone();
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

    #[test]
    fn curves_pass_quiet_audio_and_bend_loud_audio() {
        for curve in SaturationCurve::ALL {
            let small = curve.shape(0.001);
            assert!((small - 0.001).abs() < 1e-5, "{curve:?}: {small}");
            assert!(curve.shape(10.0) < 2.0, "{curve:?}");
        }
        assert_eq!(SaturationCurve::HardClip.shape(-3.0), -1.0);
        // Tape leans: its halves differ.
        let tape = SaturationCurve::Tape;
        assert!((tape.shape(2.0) + tape.shape(-2.0)).abs() > 0.1);
    }
}
