//! The gain curves of clip fades.

/// How a fade's gain rises, before its curvature.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FadeShape {
    #[default]
    Linear,
    /// A quarter of a sine: two of them crossing keep the power constant,
    /// so a crossfade doesn't dip.
    EqualPower,
    /// Even in decibels: slow to rise, quick at the end.
    Exponential,
}

impl FadeShape {
    pub const ALL: [Self; 3] = [Self::Linear, Self::EqualPower, Self::Exponential];
}

/// How steep [`FadeShape::Exponential`] is: about 35 dB up its range.
const EXPONENT: f32 = 4.;
/// How far a curvature of 1 bends a fade, the gain half way through being
/// the shape's at `0.5 + BEND` rather than at 0.5. Short of 0.5, which would
/// make it a step.
const BEND: f32 = 0.45;

/// A fade's gain from silence to unity: its shape, bent by its curvature.
#[derive(Clone, Copy, Debug)]
pub struct FadeCurve {
    pub shape: FadeShape,
    /// -1 to 1: 0 is the shape as it is, above it the gain rises sooner,
    /// below it later.
    pub curvature: f32,
}

impl FadeCurve {
    pub const LINEAR: Self = Self::of(FadeShape::Linear);
    pub const EQUAL_POWER: Self = Self::of(FadeShape::EqualPower);

    pub const fn of(shape: FadeShape) -> Self {
        Self {
            shape,
            curvature: 0.,
        }
    }

    /// The same shape, its curvature in range (anything not a number being
    /// none).
    pub fn with_curvature(self, curvature: f32) -> Self {
        Self {
            curvature: if curvature.is_nan() {
                0.
            } else {
                curvature.clamp(-1., 1.)
            },
            ..self
        }
    }

    /// The gain `t` of the way through (0 to 1), 0 at the start and 1 at the
    /// end.
    #[inline]
    pub fn gain(self, t: f32) -> f32 {
        let t = bend(t.clamp(0., 1.), self.curvature);
        match self.shape {
            FadeShape::Linear => t,
            FadeShape::EqualPower => (t * std::f32::consts::FRAC_PI_2).sin(),
            FadeShape::Exponential => (EXPONENT * t).exp_m1() / EXPONENT.exp_m1(),
        }
    }

    /// The curve that goes with this one across a crossfade: played
    /// backwards as a fade out, while this one fades in, its bend is the
    /// exact complement of this one's. Equal power stays equal power, however
    /// curved, and linear still adds up to unity.
    pub fn crossing(self) -> Self {
        Self {
            curvature: -self.curvature,
            ..self
        }
    }
}

/// `t` (0 to 1) moved towards 1 by `curvature`, 0 and 1 staying where they
/// are. A rational curve rather than a power: `1 - bend(1 - t, c)` is
/// `bend(t, -c)`, which is what makes [`FadeCurve::crossing`] exact.
#[inline]
fn bend(t: f32, curvature: f32) -> f32 {
    let half = 0.5 + BEND * curvature.clamp(-1., 1.);
    t / ((1. / half - 2.) * (1. - t) + 1.)
}

// By their bits, so curves can identify a node's content.
impl PartialEq for FadeCurve {
    fn eq(&self, other: &Self) -> bool {
        self.shape == other.shape && self.curvature.to_bits() == other.curvature.to_bits()
    }
}

impl Eq for FadeCurve {}

impl std::hash::Hash for FadeCurve {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.shape.hash(state);
        self.curvature.to_bits().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn every_curve_goes_from_silence_to_unity_only_rising() {
        for shape in FadeShape::ALL {
            for curvature in [-1., -0.3, 0., 0.3, 1.] {
                let curve = FadeCurve::of(shape).with_curvature(curvature);
                assert!(close(curve.gain(0.), 0.), "{curve:?}");
                assert!(close(curve.gain(1.), 1.), "{curve:?}");
                let mut last = 0.;
                for i in 1..=100 {
                    let gain = curve.gain(i as f32 / 100.);
                    assert!(gain > last, "{curve:?} at {i}");
                    last = gain;
                }
            }
        }
    }

    #[test]
    fn shapes_have_their_gain_half_way() {
        assert!(close(FadeCurve::LINEAR.gain(0.5), 0.5));
        assert!(close(
            FadeCurve::EQUAL_POWER.gain(0.5),
            std::f32::consts::FRAC_1_SQRT_2
        ));
        let exponential = FadeCurve::of(FadeShape::Exponential).gain(0.5);
        assert!(close(exponential, 1. / ((EXPONENT / 2.).exp() + 1.)));
    }

    #[test]
    fn curvature_bends_the_gain_up_or_down() {
        let curve = |c| FadeCurve::LINEAR.with_curvature(c).gain(0.5);
        assert!(close(curve(1.), 0.5 + BEND));
        assert!(close(curve(-1.), 0.5 - BEND));
        assert!(curve(0.5) > curve(0.));
    }

    /// Whatever its curvature, a crossfade holds what it's meant to: the
    /// power across equal power, the gain across linear.
    #[test]
    fn crossing_curves_complement_each_other() {
        for curvature in [-1., -0.4, 0., 0.7, 1.] {
            let power = FadeCurve::EQUAL_POWER.with_curvature(curvature);
            let linear = FadeCurve::LINEAR.with_curvature(curvature);
            for i in 0..=20 {
                let t = i as f32 / 20.;
                let (a, b) = (power.gain(t), power.crossing().gain(1. - t));
                assert!(close(a * a + b * b, 1.), "{curvature} at {t}");
                let (a, b) = (linear.gain(t), linear.crossing().gain(1. - t));
                assert!(close(a + b, 1.), "{curvature} at {t}");
            }
        }
    }

    #[test]
    fn curvatures_stay_in_range() {
        let curve = |c| FadeCurve::LINEAR.with_curvature(c).curvature;
        assert_eq!(curve(3.), 1.);
        assert_eq!(curve(-3.), -1.);
        assert_eq!(curve(f32::NAN), 0.);
    }
}
