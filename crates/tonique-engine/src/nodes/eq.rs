//! Parametric EQ: eight bands in series, each any shape of filter.

use std::sync::Arc;

use crate::dsp::{Biquad, BiquadState, FilterMode};
use crate::graph::{Node, NodeIdentity, NodeProperties, ProcessContext, StateTransfer};
use crate::nodes::fx::COEF_UPDATE_INTERVAL;
use crate::param::{AtomicParam, Smoother};

/// One of an EQ's bands: how it starts, and the names of its parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EqBand {
    /// Shape of a new EQ's band.
    pub mode: FilterMode,
    /// Frequency of a new EQ's band, in Hz.
    pub hz: f32,
    /// Frequency (Hz), gain (dB), Q, shape (its place in
    /// [`FilterMode::ALL`]) and on (0 or 1).
    pub params: [&'static str; 5],
}

macro_rules! band {
    ($name:literal, $mode:ident, $hz:literal) => {
        EqBand {
            mode: FilterMode::$mode,
            hz: $hz,
            params: [
                concat!($name, "_freq"),
                concat!($name, "_gain"),
                concat!($name, "_q"),
                concat!($name, "_type"),
                concat!($name, "_on"),
            ],
        }
    };
}

/// An EQ's bands. All start off, spread low to high with the shapes
/// usual there; any can take any shape.
pub const EQ_BANDS: [EqBand; 8] = [
    band!("b1", HighPass, 30.0),
    band!("b2", LowShelf, 100.0),
    band!("b3", Peak, 250.0),
    band!("b4", Peak, 600.0),
    band!("b5", Peak, 1500.0),
    band!("b6", Peak, 4000.0),
    band!("b7", HighShelf, 8000.0),
    band!("b8", LowPass, 18000.0),
];

/// Gain range of a band, in dB.
pub const EQ_GAIN_DB: f32 = 18.0;

/// The shape a band's shape parameter stands for.
pub fn eq_mode(value: f32) -> FilterMode {
    FilterMode::ALL[(value.round().max(0.0) as usize).min(FilterMode::ALL.len() - 1)]
}

/// The shape parameter's value for `mode`.
pub fn eq_mode_value(mode: FilterMode) -> f32 {
    FilterMode::ALL.iter().position(|m| *m == mode).unwrap_or(0) as f32
}

const BANDS: usize = EQ_BANDS.len();
/// Samples a band crossfades over when its shape changes.
const SHAPE_FADE: usize = 256;

/// A band's filter, and the one it's fading from after a change of shape.
#[derive(Clone, Copy, Default)]
struct Band {
    mode: Option<FilterMode>,
    coefs: Biquad,
    state: [BiquadState; 2],
    old_coefs: Biquad,
    old_state: [BiquadState; 2],
    /// Samples of the crossfade left.
    fading: usize,
}

/// Stereo EQ of [`EQ_BANDS`]. Each band runs whether on or not and is
/// faded in and out, so switching it doesn't click; a change of shape is
/// crossfaded too.
pub struct EqNode {
    /// Per band: frequency, gain, Q, shape, on.
    params: [[Arc<AtomicParam>; 5]; BANDS],
    /// Per band: frequency, gain, Q, on. The shape isn't ramped.
    smoothers: [[Smoother; 4]; BANDS],
    bands: [Band; BANDS],
    sample_rate: f32,
    identity: Option<NodeIdentity>,
}

impl EqNode {
    /// `params` in the order of [`EQ_BANDS`] and of their names.
    pub fn new(params: [[Arc<AtomicParam>; 5]; BANDS]) -> Self {
        Self {
            smoothers: std::array::from_fn(|b| {
                [0, 1, 2, 4].map(|i| Smoother::new(params[b][i].get()))
            }),
            params,
            bands: [Band::default(); BANDS],
            sample_rate: 48000.0,
            identity: None,
        }
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }

    fn smoothed(&self, band: usize) -> [&Arc<AtomicParam>; 4] {
        let [hz, gain, q, _, on] = &self.params[band];
        [hz, gain, q, on]
    }

    /// Work the band's coefficients out again; if its shape changed, start
    /// fading from the old filter.
    fn update(&mut self, band: usize) {
        let mode = eq_mode(self.params[band][3].get());
        let [hz, gain, q, _] = self.smoothers[band].each_ref().map(|s| s.current());
        let coefs = Biquad::new(mode, hz, q, gain, self.sample_rate);
        let b = &mut self.bands[band];
        if b.mode.is_some_and(|m| m != mode) {
            b.old_coefs = b.coefs;
            b.old_state = b.state;
            b.state = Default::default();
            b.fading = SHAPE_FADE;
        }
        b.mode = Some(mode);
        b.coefs = coefs;
    }
}

impl Node for EqNode {
    fn properties(&self) -> NodeProperties {
        let p = NodeProperties::audio(2);
        match self.identity {
            Some(id) => p.with_identity(id),
            None => p,
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block: usize) {
        self.sample_rate = sample_rate as f32;
        for band in 0..BANDS {
            let params = self.smoothed(band).map(Arc::clone);
            for (s, p) in self.smoothers[band].iter_mut().zip(&params) {
                s.set_sample_rate(sample_rate);
                s.snap(p);
            }
            self.update(band);
        }
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        ctx.sum_inputs_to_output();
        for band in 0..BANDS {
            let [hz, gain, q, _, on] = &self.params[band];
            for (s, p) in self.smoothers[band].iter_mut().zip([hz, gain, q, on]) {
                s.retarget(p);
            }
        }
        let len = ctx.block_len;
        let mut start = 0;
        while start < len {
            let end = (start + COEF_UPDATE_INTERVAL).min(len);
            for band in 0..BANDS {
                // The switch is followed per sample, the rest per chunk.
                let [hz, gain, q, _] = &mut self.smoothers[band];
                let ramping = hz.is_ramping() || gain.is_ramping() || q.is_ramping();
                if ramping {
                    for _ in start..end {
                        hz.next();
                        gain.next();
                        q.next();
                    }
                }
                let reshaped = self.bands[band].mode != Some(eq_mode(self.params[band][3].get()));
                if ramping || reshaped {
                    self.update(band);
                }
                let on = &mut self.smoothers[band][3];
                let from = on.clone();
                let b = &mut self.bands[band];
                let fading = b.fading;
                for ch in 0..2 {
                    // Both channels follow the same ramps.
                    *on = from.clone();
                    let mut left = fading;
                    for x in &mut ctx.audio_out.channel_mut(ch)[start..end] {
                        let mix = on.next().clamp(0.0, 1.0);
                        // Run while off too, to keep its memory current.
                        let mut y = b.state[ch].process(&b.coefs, *x);
                        if left > 0 {
                            let old = b.old_state[ch].process(&b.old_coefs, *x);
                            let t = left as f32 / SHAPE_FADE as f32;
                            y += (old - y) * t;
                            left -= 1;
                        }
                        *x += (y - *x) * mix;
                    }
                    if ch == 1 {
                        b.fading = left;
                    }
                }
            }
            start = end;
        }
    }

    fn take_state_from(&mut self, previous: &mut dyn Node) -> StateTransfer {
        if let Some(prev) = previous.as_any_mut().and_then(|a| a.downcast_mut::<Self>()) {
            self.bands = prev.bands;
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
    fn shapes_are_saved_by_place() {
        for mode in FilterMode::ALL {
            assert_eq!(eq_mode(eq_mode_value(mode)), mode);
        }
        assert_eq!(eq_mode(99.0), FilterMode::Peak);
        assert_eq!(eq_mode(-1.0), FilterMode::LowPass);
    }
}
