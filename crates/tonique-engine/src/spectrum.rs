//! Spectrum analyser for displays, steady enough to read: a long FFT,
//! levels that rise fast and fall slowly, and a curve that's smooth at any
//! zoom. Runs on the UI thread; it lives here to be optimised in debug
//! builds along with the rest of the DSP.

use rustfft::{Fft, FftPlanner, num_complex::Complex};
use std::sync::Arc;

use crate::meter::{ChannelMeter, TAP_SCOPE_LEN};

/// Samples per analysis: 5.9 Hz bins at 48 kHz, fine enough for the lows.
pub const FFT_SIZE: usize = TAP_SCOPE_LEN;
/// Time for a level to cover ~63% of the way to a louder one, in seconds.
const RISE: f32 = 0.02;
/// The same, falling to a quieter one: slower, so the curve doesn't flicker.
const FALL: f32 = 0.3;
/// Width of the bands shown, in octaves.
const BANDWIDTH: f32 = 1. / 6.;
/// How far below its level a bin's peak has fallen half a band away, in dB.
const PEAK_FALLOFF: f32 = 6.;
/// Silence, in dBFS.
pub const FLOOR_DB: f32 = -120.;
/// Analyses per second, at most: a display needs no more, and the curve
/// is smoothed over time anyway.
const RATE: f32 = 30.;
/// The envelope is kept on a grid this fine, in octaves, from this
/// frequency up to Nyquist.
const GRID_STEP: f32 = 1. / 96.;
const GRID_FROM_HZ: f32 = 10.;

pub struct Spectrum {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    /// Scales a bin's magnitude so a full-scale sine reads 0 dBFS.
    scale: f32,
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    samples: [Vec<f32>; 2],
    /// Level of each bin, smoothed over time, in dBFS.
    bins: Vec<f32>,
    sample_rate: f32,
    /// The grid cell each bin falls in (none for bin 0, DC).
    bin_cells: Vec<usize>,
    /// The loudest bin in each grid cell.
    cells: Vec<f32>,
    /// The bins as rounded peaks, a band wide: their outline on the grid.
    envelope: Vec<f32>,
    /// How far below a peak its outline is, `i` cells away.
    falloff: Vec<f32>,
    /// Time since the last analysis.
    pending: f32,
}

impl Default for Spectrum {
    fn default() -> Self {
        Self::new()
    }
}

impl Spectrum {
    pub fn new() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(FFT_SIZE);
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                0.5 * (1. - (2. * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos())
            })
            .collect();
        let scale = 2. / window.iter().sum::<f32>();
        // Three half-bands out, a peak has fallen 54 dB: far enough.
        let half_band = BANDWIDTH / 2.;
        let reach = (3. * half_band / GRID_STEP).ceil() as usize;
        let falloff = (0..=reach)
            .map(|i| {
                let distance = i as f32 * GRID_STEP / half_band;
                PEAK_FALLOFF * distance * distance
            })
            .collect();
        Self {
            scratch: vec![Complex::default(); fft.get_inplace_scratch_len()],
            fft,
            window,
            scale,
            buffer: vec![Complex::default(); FFT_SIZE],
            samples: [vec![0.; FFT_SIZE], vec![0.; FFT_SIZE]],
            bins: vec![FLOOR_DB; FFT_SIZE / 2],
            sample_rate: 0.,
            bin_cells: Vec::new(),
            cells: Vec::new(),
            envelope: Vec::new(),
            falloff,
            pending: f32::INFINITY,
        }
    }

    /// Fold in the latest samples of `meter` (both channels mixed), `dt`
    /// seconds after the previous update. Analyses at most [`RATE`] times
    /// a second; in between, this is free.
    pub fn update(&mut self, meter: &ChannelMeter, sample_rate: f32, dt: f32) {
        self.set_sample_rate(sample_rate);
        self.pending += dt;
        if self.pending < 1. / RATE {
            return;
        }
        let [left, right] = &mut self.samples;
        meter.read_scope(0, left);
        meter.read_scope(1, right);
        // The first analysis jumps to the levels (a step long enough to).
        let dt = if self.pending.is_finite() {
            self.pending
        } else {
            f32::MAX
        };
        self.pending = 0.;
        self.analyse(dt);
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        if sample_rate == self.sample_rate {
            return;
        }
        self.sample_rate = sample_rate;
        let cells = self.cell(sample_rate / 2.).ceil() as usize + 1;
        self.bin_cells = (0..self.bins.len())
            .map(|k| {
                let hz = k as f32 * sample_rate / FFT_SIZE as f32;
                (self.cell(hz).round().max(0.) as usize).min(cells - 1)
            })
            .collect();
        self.cells = vec![FLOOR_DB; cells];
        self.envelope = vec![FLOOR_DB; cells];
    }

    /// Where `hz` is on the grid, in cells.
    fn cell(&self, hz: f32) -> f32 {
        (hz / GRID_FROM_HZ).log2() / GRID_STEP
    }

    fn analyse(&mut self, dt: f32) {
        for (i, c) in self.buffer.iter_mut().enumerate() {
            let mono = (self.samples[0][i] + self.samples[1][i]) * 0.5;
            *c = Complex::new(mono * self.window[i], 0.);
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        let dt = dt.max(0.);
        let (rise, fall) = (1. - (-dt / RISE).exp(), 1. - (-dt / FALL).exp());
        for (level, c) in self.bins.iter_mut().zip(&self.buffer) {
            // 10·log10 of the power: one log, no square root.
            let db = (10. * (c.norm_sqr() * self.scale * self.scale).log10()).max(FLOOR_DB);
            *level += (db - *level) * if db > *level { rise } else { fall };
        }

        // The outline: each cell's loudest bin, spread as a rounded peak.
        self.cells.fill(FLOOR_DB);
        for (&cell, &level) in self.bin_cells.iter().zip(&self.bins).skip(1) {
            self.cells[cell] = self.cells[cell].max(level);
        }
        self.envelope.fill(FLOOR_DB);
        let last = self.cells.len() - 1;
        for (source, &level) in self.cells.iter().enumerate() {
            if level <= FLOOR_DB {
                continue;
            }
            let reach = self.falloff.len() - 1;
            for cell in source.saturating_sub(reach)..=(source + reach).min(last) {
                let peak = level - self.falloff[cell.abs_diff(source)];
                if peak > self.envelope[cell] {
                    self.envelope[cell] = peak;
                }
            }
        }
    }

    /// Level around `hz`, in dBFS. The bins joined by a spline, under the
    /// outline of each bin as a rounded peak spreading over the band
    /// around it. Taking the loudest bin of a band instead draws flat
    /// steps, as neighbouring points share it; averaging would lower a
    /// pure tone. This way a tone still reads its level, and noise reads as
    /// a smooth outline. Cheap: the outline is worked out once per
    /// analysis.
    pub fn level(&self, hz: f32) -> f32 {
        if self.envelope.is_empty() {
            return FLOOR_DB;
        }
        let last = self.bins.len() - 1;
        let at = (hz * FFT_SIZE as f32 / self.sample_rate).clamp(1., last as f32);
        let cell = self.cell(hz).clamp(0., (self.envelope.len() - 1) as f32);
        let (i, t) = (cell.floor() as usize, cell.fract());
        let next = (i + 1).min(self.envelope.len() - 1);
        let envelope = self.envelope[i] + (self.envelope[next] - self.envelope[i]) * t;
        self.spline(at).max(envelope)
    }

    /// The bins around fractional bin `at`, joined by a Catmull-Rom spline.
    /// It may rise a little above both bins: a tone between two bins is
    /// louder than either reads.
    fn spline(&self, at: f32) -> f32 {
        let last = self.bins.len() as isize - 1;
        let i = at.floor() as isize;
        let p = |k: isize| self.bins[k.clamp(0, last) as usize];
        let (p0, p1, p2, p3) = (p(i - 1), p(i), p(i + 1), p(i + 2));
        let t = at.fract();
        let v = 0.5
            * (2. * p1
                + (p2 - p0) * t
                + (2. * p0 - 5. * p1 + 4. * p2 - p3) * t * t
                + (3. * p1 - p0 - 3. * p2 + p3) * t * t * t);
        v.clamp(FLOOR_DB, p1.max(p2) + 3.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.;

    fn spectrum() -> Spectrum {
        let mut spectrum = Spectrum::new();
        spectrum.set_sample_rate(SR);
        spectrum
    }

    fn sine(spectrum: &mut Spectrum, hz: f32, amplitude: f32) {
        for channel in &mut spectrum.samples {
            for (i, s) in channel.iter_mut().enumerate() {
                *s = amplitude * (2. * std::f32::consts::PI * hz * i as f32 / SR).sin();
            }
        }
    }

    #[test]
    fn a_sine_reads_its_level_at_its_frequency() {
        let mut spectrum = spectrum();
        for hz in [60., 1000., 12_000.] {
            sine(&mut spectrum, hz, 0.5);
            spectrum.analyse(1.);
            let db = spectrum.level(hz);
            assert!((db + 6.02).abs() < 1.5, "{hz} Hz: {db} dB");
            assert!(spectrum.level(hz * 4.) < db - 40., "{hz} Hz leaks");
        }
    }

    #[test]
    fn levels_rise_fast_and_fall_slowly() {
        let mut spectrum = spectrum();
        sine(&mut spectrum, 1000., 1.);
        spectrum.analyse(0.05);
        let risen = spectrum.level(1000.);
        assert!(risen > -15., "most of the way up in 50 ms: {risen}");

        spectrum.analyse(1.);
        sine(&mut spectrum, 1000., 0.);
        spectrum.analyse(0.05);
        let fallen = spectrum.level(1000.);
        assert!(fallen > -30., "still falling after 50 ms: {fallen}");
    }

    #[test]
    fn low_bands_are_interpolated_between_bins() {
        let mut spectrum = spectrum();
        sine(&mut spectrum, 50., 0.5);
        spectrum.analyse(1.);
        // Close frequencies, within a bin: no steps.
        let a = spectrum.level(48.);
        let b = spectrum.level(49.);
        let c = spectrum.level(50.);
        assert!(a != b && b != c, "{a} {b} {c}");
    }

    /// Noise draws a curve, not flat steps: neighbouring points differ.
    #[test]
    fn noise_draws_without_flat_steps() {
        let mut spectrum = spectrum();
        // Deterministic noise.
        let mut x = 1u32;
        for channel in &mut spectrum.samples {
            for s in channel.iter_mut() {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                *s = x as f32 / u32::MAX as f32 - 0.5;
            }
        }
        spectrum.analyse(1.);
        // A point per pixel over 20 Hz to 20 kHz, 300 pixels wide.
        let levels: Vec<f32> = (0..300)
            .map(|i| spectrum.level(20. * 1000f32.powf(i as f32 / 299.)))
            .collect();
        let flat = levels.windows(2).filter(|w| w[0] == w[1]).count();
        assert!(flat < 3, "{flat} flat steps");
    }

    /// Analyses run at most [`RATE`] times a second, however often the
    /// display asks.
    #[test]
    fn analyses_are_rate_limited() {
        let meter = ChannelMeter::with_scope_len(FFT_SIZE);
        let mut buffer = crate::audio::AudioBuffer::new(2, FFT_SIZE);
        for ch in 0..2 {
            for (i, s) in buffer.channel_mut(ch).iter_mut().enumerate() {
                *s = (2. * std::f32::consts::PI * 1000. * i as f32 / SR).sin();
            }
        }
        meter.record(&buffer.block(FFT_SIZE));
        let mut spectrum = Spectrum::new();
        spectrum.update(&meter, SR, 0.);
        assert!(spectrum.level(1000.) > -20., "the first update analyses");

        meter.record(&crate::audio::AudioBuffer::new(2, FFT_SIZE).block(FFT_SIZE));
        let before = spectrum.level(1000.);
        spectrum.update(&meter, SR, 0.01);
        assert_eq!(spectrum.level(1000.), before, "too soon: not analysed");
        spectrum.update(&meter, SR, 0.03);
        assert!(spectrum.level(1000.) < before, "analysed, falling");
    }

    /// Time per analysis and per 320-pixel curve.
    #[test]
    #[ignore]
    fn bench() {
        let mut spectrum = spectrum();
        let mut x = 1u32;
        for channel in &mut spectrum.samples {
            for s in channel.iter_mut() {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                *s = x as f32 / u32::MAX as f32 - 0.5;
            }
        }
        let start = std::time::Instant::now();
        for _ in 0..100 {
            spectrum.analyse(0.016);
        }
        let analyse = start.elapsed() / 100;
        let start = std::time::Instant::now();
        let mut sum = 0.;
        for _ in 0..100 {
            for i in 0..320 {
                sum += spectrum.level(20. * 1000f32.powf(i as f32 / 319.));
            }
        }
        eprintln!(
            "analyse {analyse:?}, curve {:?} ({sum})",
            start.elapsed() / 100
        );
    }
}
