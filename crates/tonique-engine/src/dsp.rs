//! DSP building blocks shared by the effects: biquads, level conversions,
//! time constants and an oversampler. All of it runs on the RT thread once
//! made: nothing here allocates outside `new`.

use std::f64::consts::PI;

/// Amplitude for a level in dB.
#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Level in dB of an amplitude, floored far below hearing.
#[inline]
pub fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.max(1e-10).log10()
}

/// One-pole coefficient reaching ~63% of a step in `ms`.
pub fn time_coef(ms: f32, sample_rate: f32) -> f32 {
    let samples = ms.max(0.001) * 0.001 * sample_rate;
    (-1.0 / samples).exp()
}

/// Shapes a biquad can take: the RBJ cookbook's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FilterMode {
    LowPass,
    HighPass,
    /// Passes a band around the frequency, 0 dB at its centre.
    BandPass,
    /// Cuts a band around the frequency.
    Notch,
    /// Raises or lowers what's below the frequency by the gain.
    LowShelf,
    /// Raises or lowers what's above the frequency by the gain.
    HighShelf,
    /// Raises or lowers a band around the frequency by the gain.
    Peak,
}

impl FilterMode {
    pub const ALL: [FilterMode; 7] = [
        FilterMode::LowPass,
        FilterMode::HighPass,
        FilterMode::BandPass,
        FilterMode::Notch,
        FilterMode::LowShelf,
        FilterMode::HighShelf,
        FilterMode::Peak,
    ];

    /// Whether its gain changes anything.
    pub fn has_gain(self) -> bool {
        matches!(
            self,
            FilterMode::LowShelf | FilterMode::HighShelf | FilterMode::Peak
        )
    }
}

/// Normalised biquad coefficients (a0 = 1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Biquad {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Biquad {
    /// Identity: passes audio as is.
    pub const THROUGH: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    /// RBJ cookbook filter at `hz` (kept below Nyquist), with `gain_db`
    /// for the shelves and peak.
    pub fn new(mode: FilterMode, hz: f32, q: f32, gain_db: f32, sample_rate: f32) -> Self {
        let [b0, b1, b2, a0, a1, a2] = coefs(mode, hz, q, gain_db, sample_rate);
        Self {
            b0: (b0 / a0) as f32,
            b1: (b1 / a0) as f32,
            b2: (b2 / a0) as f32,
            a1: (a1 / a0) as f32,
            a2: (a2 / a0) as f32,
        }
    }

    /// Its gain at `hz`, in dB: what [`BiquadState::process`] does to a
    /// sine there.
    pub fn response_db(&self, hz: f32, sample_rate: f32) -> f32 {
        let w = 2.0 * PI * hz as f64 / sample_rate as f64;
        // H(e^jw), with z^-1 = e^-jw.
        let magnitude = |c0: f32, c1: f32, c2: f32| {
            let (c0, c1, c2) = (c0 as f64, c1 as f64, c2 as f64);
            let re = c0 + c1 * w.cos() + c2 * (2.0 * w).cos();
            let im = -(c1 * w.sin() + c2 * (2.0 * w).sin());
            (re * re + im * im).sqrt()
        };
        let ratio = magnitude(self.b0, self.b1, self.b2) / magnitude(1.0, self.a1, self.a2);
        (20.0 * ratio.max(1e-9).log10()) as f32
    }
}

/// Unnormalised coefficients, in f64 so low frequencies stay exact.
fn coefs(mode: FilterMode, hz: f32, q: f32, gain_db: f32, sample_rate: f32) -> [f64; 6] {
    let sr = sample_rate as f64;
    let f = (hz as f64).clamp(10.0, sr * 0.49);
    let w = 2.0 * PI * f / sr;
    let (sin, cos) = w.sin_cos();
    let alpha = sin / (2.0 * (q as f64).max(0.05));
    let a = 10f64.powf(gain_db as f64 / 40.0);
    let sq = 2.0 * a.sqrt() * alpha;
    match mode {
        FilterMode::LowPass => [
            (1.0 - cos) / 2.0,
            1.0 - cos,
            (1.0 - cos) / 2.0,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        ],
        FilterMode::HighPass => [
            (1.0 + cos) / 2.0,
            -(1.0 + cos),
            (1.0 + cos) / 2.0,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        ],
        FilterMode::BandPass => [alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha],
        FilterMode::Notch => [1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha],
        FilterMode::Peak => [
            1.0 + alpha * a,
            -2.0 * cos,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cos,
            1.0 - alpha / a,
        ],
        FilterMode::LowShelf => [
            a * ((a + 1.0) - (a - 1.0) * cos + sq),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - sq),
            (a + 1.0) + (a - 1.0) * cos + sq,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - sq,
        ],
        FilterMode::HighShelf => [
            a * ((a + 1.0) + (a - 1.0) * cos + sq),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
            a * ((a + 1.0) + (a - 1.0) * cos - sq),
            (a + 1.0) - (a - 1.0) * cos + sq,
            2.0 * ((a - 1.0) - (a + 1.0) * cos),
            (a + 1.0) - (a - 1.0) * cos - sq,
        ],
    }
}

/// Memory of one channel through a [`Biquad`] (direct form I).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BiquadState {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BiquadState {
    #[inline]
    pub fn process(&mut self, k: &Biquad, x: f32) -> f32 {
        let y = k.b0 * x + k.b1 * self.x1 + k.b2 * self.x2 - k.a1 * self.y1 - k.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        // Flush what's too small to hear before it turns denormal.
        self.y1 = if y.abs() < 1e-20 { 0.0 } else { y };
        y
    }
}

/// Taps of the oversampler's filter at the high rate: `4 * DELAY + 1`.
const OVERSAMPLER_TAPS: usize = 97;

/// Runs a function at 4× the sample rate: up-sample, process, low-pass,
/// down-sample. What the function adds above the original Nyquist is
/// filtered out instead of folding back as aliasing. Delays by
/// [`Oversampler::LATENCY`] samples.
pub struct Oversampler {
    /// Windowed-sinc low-pass, shared by both directions.
    kernel: [f32; OVERSAMPLER_TAPS],
    /// Recent input samples, newest at `pos` (doubled to read straight).
    input: [f32; 2 * Self::INPUT_LEN],
    in_pos: usize,
    /// Recent high-rate samples, newest at `pos`.
    high: [f32; 2 * OVERSAMPLER_TAPS],
    high_pos: usize,
}

impl Oversampler {
    pub const FACTOR: usize = 4;
    /// Delay of a sample through, at the original rate.
    pub const LATENCY: usize = (OVERSAMPLER_TAPS - 1) / Self::FACTOR;
    /// Input samples under the kernel at once.
    const INPUT_LEN: usize = OVERSAMPLER_TAPS.div_ceil(Self::FACTOR);

    pub fn new() -> Self {
        // Cut just below the original Nyquist (0.125 of the high rate):
        // what's left above folds back above 20 kHz at 44.1 kHz.
        let cutoff = 0.11f64;
        let mid = (OVERSAMPLER_TAPS - 1) as f64 / 2.0;
        let mut kernel = [0f32; OVERSAMPLER_TAPS];
        let mut sum = 0.0;
        for (i, k) in kernel.iter_mut().enumerate() {
            let t = i as f64 - mid;
            let sinc = if t == 0.0 {
                2.0 * cutoff
            } else {
                (2.0 * PI * cutoff * t).sin() / (PI * t)
            };
            // Blackman window.
            let x = i as f64 / (OVERSAMPLER_TAPS - 1) as f64;
            let window = 0.42 - 0.5 * (2.0 * PI * x).cos() + 0.08 * (4.0 * PI * x).cos();
            *k = (sinc * window) as f32;
            sum += *k as f64;
        }
        for k in &mut kernel {
            *k = (*k as f64 / sum) as f32;
        }
        Self {
            kernel,
            input: [0.0; 2 * Self::INPUT_LEN],
            in_pos: 0,
            high: [0.0; 2 * OVERSAMPLER_TAPS],
            high_pos: 0,
        }
    }

    /// Forget past audio.
    pub fn reset(&mut self) {
        self.input.fill(0.0);
        self.high.fill(0.0);
    }

    /// One sample through `f` at the high rate; returns the sample from
    /// [`Self::LATENCY`] samples ago.
    #[inline]
    pub fn process(&mut self, x: f32, mut f: impl FnMut(f32) -> f32) -> f32 {
        // Newest first from `in_pos`.
        self.in_pos = (self.in_pos + Self::INPUT_LEN - 1) % Self::INPUT_LEN;
        self.input[self.in_pos] = x;
        self.input[self.in_pos + Self::INPUT_LEN] = x;
        let recent = &self.input[self.in_pos..self.in_pos + Self::INPUT_LEN];
        let mut out = 0.0;
        for phase in 0..Self::FACTOR {
            // Zero-stuffed: only every FACTOR-th tap meets a sample. Scaled
            // by FACTOR to make up for the zeros.
            let up: f32 = self.kernel[phase..]
                .iter()
                .step_by(Self::FACTOR)
                .zip(recent)
                .map(|(k, s)| k * s)
                .sum::<f32>()
                * Self::FACTOR as f32;
            let y = f(up);
            self.high_pos = (self.high_pos + OVERSAMPLER_TAPS - 1) % OVERSAMPLER_TAPS;
            self.high[self.high_pos] = y;
            self.high[self.high_pos + OVERSAMPLER_TAPS] = y;
            // The first phase lines up with an input sample.
            if phase == 0 {
                out = self
                    .kernel
                    .iter()
                    .zip(&self.high[self.high_pos..self.high_pos + OVERSAMPLER_TAPS])
                    .map(|(k, s)| k * s)
                    .sum();
            }
        }
        out
    }
}

impl Default for Oversampler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// Gain of `k` on a steady sine at `hz`, measured.
    fn measured_db(k: &Biquad, hz: f32) -> f32 {
        let mut st = BiquadState::default();
        let mut peak = 0f32;
        for i in 0..48_000 {
            let x = (2.0 * std::f32::consts::PI * hz * i as f32 / SR).sin();
            let y = st.process(k, x);
            if i > 24_000 {
                peak = peak.max(y.abs());
            }
        }
        gain_to_db(peak)
    }

    #[test]
    fn responses_match_what_is_heard() {
        for mode in FilterMode::ALL {
            let k = Biquad::new(mode, 1000.0, 1.0, 6.0, SR);
            for hz in [100.0, 1000.0, 5000.0] {
                let (drawn, heard) = (k.response_db(hz, SR), measured_db(&k, hz));
                if drawn > -40.0 {
                    assert!(
                        (drawn - heard).abs() < 0.2,
                        "{mode:?} at {hz}: {drawn} vs {heard}"
                    );
                }
            }
        }
    }

    #[test]
    fn shapes_do_what_they_say() {
        let at = |mode, hz| Biquad::new(mode, 1000.0, 0.707, 12.0, SR).response_db(hz, SR);
        assert!(at(FilterMode::LowShelf, 30.0) > 11.5);
        assert!(at(FilterMode::LowShelf, 15_000.0).abs() < 0.2);
        assert!(at(FilterMode::HighShelf, 15_000.0) > 11.5);
        assert!(at(FilterMode::HighShelf, 30.0).abs() < 0.2);
        assert!((at(FilterMode::Peak, 1000.0) - 12.0).abs() < 0.01);
        assert!(at(FilterMode::Peak, 30.0).abs() < 0.2);
        assert!(at(FilterMode::BandPass, 1000.0).abs() < 0.01);
        assert!(at(FilterMode::BandPass, 50.0) < -20.0);
        assert!(at(FilterMode::Notch, 1000.0) < -60.0);
        assert!(at(FilterMode::Notch, 50.0).abs() < 0.1);
        // No gain, no change.
        let flat = Biquad::new(FilterMode::Peak, 1000.0, 2.0, 0.0, SR);
        assert!(flat.response_db(1000.0, SR).abs() < 1e-4);
    }

    #[test]
    fn oversampler_passes_the_audible_band_delayed() {
        let mut os = Oversampler::new();
        let hz = 1000.0;
        let sine = |i: usize| (2.0 * std::f32::consts::PI * hz * i as f32 / SR).sin();
        let mut err = 0f32;
        for i in 0..4800 {
            let y = os.process(sine(i), |x| x);
            if i > 200 {
                err = err.max((y - sine(i - Oversampler::LATENCY)).abs());
            }
        }
        assert!(err < 0.01, "{err}");
    }

    #[test]
    fn oversampler_keeps_aliases_out() {
        // A hard-clipped 5 kHz sine: its harmonics go far past Nyquist.
        // Folded back, they'd land between the true ones.
        let (hz, n) = (5000.0, 1 << 14);
        let run = |oversample: bool| {
            let mut os = Oversampler::new();
            let clip = |x: f32| (4.0 * x).clamp(-1.0, 1.0);
            let out: Vec<f32> = (0..n)
                .map(|i| {
                    let x = (2.0 * std::f32::consts::PI * hz * i as f32 / SR).sin();
                    if oversample {
                        os.process(x, clip)
                    } else {
                        clip(x)
                    }
                })
                .collect();
            // Energy at 1 kHz: not a harmonic of 5 kHz, so only aliases
            // put anything there.
            let probe = 1000.0;
            let (mut re, mut im) = (0f64, 0f64);
            for (i, y) in out.iter().enumerate().skip(1000) {
                let w = 2.0 * std::f64::consts::PI * probe * i as f64 / SR as f64;
                re += *y as f64 * w.cos();
                im += *y as f64 * w.sin();
            }
            ((re * re + im * im).sqrt() / n as f64) as f32
        };
        let (naive, oversampled) = (run(false), run(true));
        assert!(
            oversampled < naive * 0.1,
            "aliasing {naive} without, {oversampled} with"
        );
    }
}
