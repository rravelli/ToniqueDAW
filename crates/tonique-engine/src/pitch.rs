//! Pitch tracker for a tuner: the McLeod pitch method (a normalised
//! autocorrelation, computed by FFT) over the latest samples, then smoothed
//! so the reading holds still on a held note. Runs on the UI thread; it
//! lives here to be optimised in debug builds along with the rest of the
//! DSP.

use rustfft::{Fft, FftPlanner, num_complex::Complex};
use std::sync::Arc;

use crate::meter::{ChannelMeter, TAP_SCOPE_LEN};

/// Lowest and highest pitches tracked, in Hz: under a 5-string bass's low
/// B to over a piano's top C.
pub const MIN_HZ: f32 = 30.;
pub const MAX_HZ: f32 = 4200.;
/// Analyses per second, at most.
const RATE: f32 = 30.;
/// Seconds of audio per analysis: two periods of the lowest pitch, and some.
const WINDOW_S: f32 = 0.085;
/// Quieter than this (RMS, dBFS), nothing is played.
const SILENCE_DB: f32 = -60.;
/// How periodic the audio must be, 0 to 1, to have a pitch: noise and
/// chords fall under it.
const MIN_CLARITY: f32 = 0.8;
/// Of the highest autocorrelation peak, how high the first must be to be
/// the period rather than a harmonic's.
const PEAK_THRESHOLD: f32 = 0.9;
/// Time for the reading to cover ~63% of the way to a new one on the same
/// note, in seconds.
const SMOOTHING: f32 = 0.08;
/// Further than this from the reading, in semitones, a pitch is a new
/// note: the reading jumps to it.
const NEW_NOTE: f32 = 0.5;
/// How long a reading stays after the pitch is lost, in seconds.
const HOLD: f32 = 0.5;

pub struct PitchTracker {
    sample_rate: f32,
    /// Samples per analysis, a power of two.
    window: usize,
    fft: Arc<dyn Fft<f32>>,
    ifft: Arc<dyn Fft<f32>>,
    /// Twice the window: the autocorrelation doesn't wrap around.
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    samples: [Vec<f32>; 2],
    mono: Vec<f32>,
    nsdf: Vec<f32>,
    /// The reading, in semitones from A4.
    reading: Option<f32>,
    /// Seconds since a pitch was last found.
    lost_for: f32,
    pending: f32,
}

impl Default for PitchTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl PitchTracker {
    pub fn new() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(1);
        Self {
            sample_rate: 0.,
            window: 0,
            ifft: fft.clone(),
            fft,
            buffer: Vec::new(),
            scratch: Vec::new(),
            samples: [Vec::new(), Vec::new()],
            mono: Vec::new(),
            nsdf: Vec::new(),
            reading: None,
            lost_for: f32::INFINITY,
            pending: f32::INFINITY,
        }
    }

    /// The pitch heard, in Hz, smoothed; `None` once nothing has had one
    /// for a moment.
    pub fn hz(&self) -> Option<f32> {
        self.reading.map(|semis| 440. * (semis / 12.).exp2())
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
        let dt = if self.pending.is_finite() {
            self.pending
        } else {
            0.
        };
        self.pending = 0.;
        let found = self.detect();
        self.follow(found, dt);
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        if sample_rate == self.sample_rate {
            return;
        }
        self.sample_rate = sample_rate;
        self.window = ((sample_rate * WINDOW_S) as usize)
            .next_power_of_two()
            .clamp(1024, TAP_SCOPE_LEN);
        let mut planner = FftPlanner::new();
        self.fft = planner.plan_fft_forward(2 * self.window);
        self.ifft = planner.plan_fft_inverse(2 * self.window);
        let scratch = self
            .fft
            .get_inplace_scratch_len()
            .max(self.ifft.get_inplace_scratch_len());
        self.scratch = vec![Complex::default(); scratch];
        self.buffer = vec![Complex::default(); 2 * self.window];
        self.samples = [vec![0.; self.window], vec![0.; self.window]];
        self.mono = vec![0.; self.window];
        self.nsdf = vec![0.; self.window / 2];
        self.reading = None;
    }

    /// Move the reading towards `found` (in Hz), over `dt` seconds.
    fn follow(&mut self, found: Option<f32>, dt: f32) {
        let Some(hz) = found else {
            self.lost_for += dt;
            if self.lost_for > HOLD {
                self.reading = None;
            }
            return;
        };
        self.lost_for = 0.;
        let semis = 12. * (hz / 440.).log2();
        self.reading = Some(match self.reading {
            Some(reading) if (semis - reading).abs() < NEW_NOTE => {
                reading + (semis - reading) * (1. - (-dt / SMOOTHING).exp())
            }
            _ => semis,
        });
    }

    /// The pitch of the samples, in Hz, if they're loud and periodic
    /// enough to have one.
    fn detect(&mut self) -> Option<f32> {
        let n = self.window;
        for (m, (l, r)) in self
            .mono
            .iter_mut()
            .zip(self.samples[0].iter().zip(&self.samples[1]))
        {
            *m = (l + r) * 0.5;
        }
        let energy: f32 = self.mono.iter().map(|s| s * s).sum();
        if 10. * (energy / n as f32).log10() < SILENCE_DB {
            return None;
        }

        // Autocorrelation r(τ) = Σ x[j]·x[j+τ], by FFT of the zero-padded
        // window.
        for (b, &s) in self
            .buffer
            .iter_mut()
            .zip(self.mono.iter().chain(std::iter::repeat(&0.)))
        {
            *b = Complex::new(s, 0.);
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        for b in &mut self.buffer {
            *b = Complex::new(b.norm_sqr(), 0.);
        }
        self.ifft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        let scale = 1. / self.buffer.len() as f32;

        // Normalised: 2·r(τ) / Σ (x[j]² + x[j+τ]²), 1 for a perfect period.
        let mut m = 2. * energy;
        for (tau, v) in self.nsdf.iter_mut().enumerate() {
            if tau > 0 {
                m -= self.mono[tau - 1].powi(2) + self.mono[n - tau].powi(2);
            }
            *v = if m > 0. {
                2. * self.buffer[tau].re * scale / m
            } else {
                0.
            };
        }

        let min_lag = ((self.sample_rate / MAX_HZ) as usize).max(2);
        let max_lag = ((self.sample_rate / MIN_HZ).ceil() as usize).min(self.nsdf.len() - 2);
        // The highest point of each lobe above zero, past the one at lag 0.
        let mut peaks: Vec<(usize, f32)> = Vec::new();
        let mut tau = 1;
        while tau < max_lag && self.nsdf[tau] > 0. {
            tau += 1;
        }
        let mut best: Option<(usize, f32)> = None;
        while tau <= max_lag {
            let v = self.nsdf[tau];
            if v > 0. {
                if best.is_none_or(|(_, b)| v > b) {
                    best = Some((tau, v));
                }
            } else if let Some(peak) = best.take() {
                peaks.push(peak);
            }
            tau += 1;
        }
        peaks.extend(best);
        let highest = peaks.iter().map(|&(_, v)| v).fold(0., f32::max);
        let &(tau, _) = peaks
            .iter()
            .find(|&&(tau, v)| tau >= min_lag && v >= PEAK_THRESHOLD * highest)?;

        // Between samples: the top of a parabola through the peak.
        let (a, b, c) = (self.nsdf[tau - 1], self.nsdf[tau], self.nsdf[tau + 1]);
        let curve = a - 2. * b + c;
        let shift = if curve < 0. {
            (a - c) / (2. * curve)
        } else {
            0.
        };
        let clarity = b - (a - c) * shift / 4.;
        if clarity < MIN_CLARITY {
            return None;
        }
        Some(self.sample_rate / (tau as f32 + shift))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    const SR: f32 = 48_000.;

    fn tracker() -> PitchTracker {
        let mut tracker = PitchTracker::new();
        tracker.set_sample_rate(SR);
        tracker
    }

    /// The tracker's samples set to `wave` of the time in seconds.
    fn play(tracker: &mut PitchTracker, wave: impl Fn(f32) -> f32) {
        for channel in &mut tracker.samples {
            for (i, s) in channel.iter_mut().enumerate() {
                *s = wave(i as f32 / SR);
            }
        }
    }

    fn cents(hz: f32, expected: f32) -> f32 {
        1200. * (hz / expected).log2()
    }

    #[test]
    fn a_sine_reads_its_pitch_within_a_cent() {
        let mut tracker = tracker();
        for hz in [41.2, 82.41, 440., 1046.5, 3951.] {
            play(&mut tracker, |t| 0.5 * (TAU * hz * t).sin());
            let found = tracker.detect().expect("a pitch");
            assert!(cents(found, hz).abs() < 1., "{hz} Hz read {found}");
        }
    }

    /// A sawtooth's harmonics are as strong as can be: it still reads its
    /// fundamental, not an octave up.
    #[test]
    fn a_rich_tone_reads_its_fundamental() {
        let mut tracker = tracker();
        for hz in [55., 110., 329.6] {
            play(&mut tracker, |t| 0.4 * ((hz * t).fract() * 2. - 1.));
            let found = tracker.detect().expect("a pitch");
            assert!(cents(found, hz).abs() < 2., "{hz} Hz read {found}");
        }
    }

    #[test]
    fn silence_and_noise_have_no_pitch() {
        let mut tracker = tracker();
        play(&mut tracker, |_| 0.);
        assert_eq!(tracker.detect(), None);
        play(&mut tracker, |t| 0.0005 * (TAU * 440. * t).sin());
        assert_eq!(tracker.detect(), None, "under the silence floor");
        let mut seed = 1u32;
        for channel in &mut tracker.samples {
            for s in channel.iter_mut() {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *s = (seed >> 8) as f32 / (1 << 24) as f32 - 0.5;
            }
        }
        assert_eq!(tracker.detect(), None, "noise");
    }

    #[test]
    fn the_reading_settles_jumps_to_new_notes_and_holds() {
        let mut tracker = tracker();
        tracker.follow(Some(440.), 0.);
        assert_eq!(tracker.hz(), Some(440.));
        // A few cents sharp: eased towards.
        tracker.follow(Some(442.), 0.02);
        let hz = tracker.hz().unwrap_or_default();
        assert!(hz > 440. && hz < 442., "{hz}");
        for _ in 0..30 {
            tracker.follow(Some(442.), 1. / RATE);
        }
        assert!(cents(tracker.hz().unwrap_or_default(), 442.).abs() < 0.1);
        // A new note: no glide through the ones between.
        tracker.follow(Some(330.), 1. / RATE);
        assert!(cents(tracker.hz().unwrap_or_default(), 330.).abs() < 0.01);
        // Lost: held a moment, then gone.
        tracker.follow(None, HOLD / 2.);
        assert!(tracker.hz().is_some());
        tracker.follow(None, HOLD);
        assert_eq!(tracker.hz(), None);
    }
}
