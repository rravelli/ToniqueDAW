//! Onset strength: how much new sound starts in each ~10 ms of a file, as
//! the spectral flux (the rise of every frequency's level since the frame
//! before). The tempo is read from the period that repeats in it
//! ([`crate::tempo`]); its peaks are where sounds start. Built off the RT
//! thread, block by block while a file decodes ([`OnsetBuilder`]).

use rustfft::{Fft, FftPlanner, num_complex::Complex};
use std::sync::Arc;

use crate::sample::SampleBuffer;

/// Samples per analysis: 23 Hz bins at 48 kHz, short enough to tell two
/// hits 10 ms apart in time from the hop alone.
pub const FFT_SIZE: usize = 2048;
/// Values of the curve per second of audio, about.
pub const VALUES_PER_SECOND: f64 = 100.;
/// How much quiet frequencies count against loud ones: magnitudes are read
/// as `ln(1 + COMPRESSION · magnitude)`, so a soft hit over a loud pad
/// still shows.
const COMPRESSION: f32 = 100.;

const MAGIC: &[u8; 4] = b"TQON";
const VERSION: u32 = 1;

/// The onset strength of a file, one value every `hop` source frames.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OnsetCurve {
    sample_rate: f64,
    /// Source frames between two values.
    hop: usize,
    /// Source frames analysed: the file's length.
    frames: usize,
    values: Vec<f32>,
}

impl OnsetCurve {
    pub fn from_buffer(buffer: &SampleBuffer) -> Self {
        let mut builder = OnsetBuilder::new(buffer.num_channels(), buffer.sample_rate());
        let channels: Vec<&[f32]> = (0..buffer.num_channels())
            .map(|channel| buffer.channel(channel))
            .collect();
        builder.push(&channels);
        builder.finish()
    }

    /// The file's sample rate.
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// Source frames between two values.
    pub fn hop(&self) -> usize {
        self.hop
    }

    /// Source frames analysed: the file's length.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Values per second.
    pub fn rate(&self) -> f64 {
        if self.hop == 0 {
            0.
        } else {
            self.sample_rate / self.hop as f64
        }
    }

    /// Strength of what starts around each value's frame (see
    /// [`Self::frame_of`]), 0 for nothing new.
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The source frame value `index` is centred on.
    pub fn frame_of(&self, index: usize) -> usize {
        index * self.hop
    }

    /// The memory its values take, in bytes.
    pub fn byte_size(&self) -> usize {
        self.values.capacity() * size_of::<f32>()
    }

    /// Compact little-endian encoding, for on-disk caches.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(40 + self.values.len() * 4);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&self.sample_rate.to_le_bytes());
        out.extend_from_slice(&(self.hop as u64).to_le_bytes());
        out.extend_from_slice(&(self.frames as u64).to_le_bytes());
        out.extend_from_slice(&(self.values.len() as u64).to_le_bytes());
        for value in &self.values {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out
    }

    /// Inverse of [`Self::to_bytes`]; `None` on any malformed input.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let (header, rest) = bytes.split_at_checked(40)?;
        let (magic, header) = header.split_at(4);
        let word = |at: usize| -> Option<[u8; 8]> { header.get(at..at + 8)?.try_into().ok() };
        let version = u32::from_le_bytes(header.get(..4)?.try_into().ok()?);
        if magic != MAGIC || version != VERSION {
            return None;
        }
        let sample_rate = f64::from_le_bytes(word(4)?);
        let hop = u64::from_le_bytes(word(12)?) as usize;
        let frames = u64::from_le_bytes(word(20)?) as usize;
        let len = u64::from_le_bytes(word(28)?) as usize;
        let (chunks, remainder) = rest.as_chunks::<4>();
        if hop == 0 || chunks.len() != len || !remainder.is_empty() {
            return None;
        }
        Some(Self {
            sample_rate,
            hop,
            frames,
            values: chunks
                .iter()
                .map(|chunk| f32::from_le_bytes(*chunk))
                .collect(),
        })
    }
}

/// Builds an [`OnsetCurve`] from planar blocks as they are decoded.
pub struct OnsetBuilder {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    /// Scales a bin's magnitude so a full-scale sine reads 1.
    scale: f32,
    num_channels: usize,
    /// Mono samples not analysed yet, starting half an analysis before the
    /// next value's frame.
    pending: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    /// Compressed magnitudes of the frame before, then of this one.
    previous: Vec<f32>,
    current: Vec<f32>,
    curve: OnsetCurve,
}

impl OnsetBuilder {
    pub fn new(num_channels: usize, sample_rate: f64) -> Self {
        let fft = FftPlanner::new().plan_fft_forward(FFT_SIZE);
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                0.5 * (1. - (2. * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos())
            })
            .collect();
        let scale = 2. / window.iter().sum::<f32>();
        let bins = FFT_SIZE / 2 + 1;
        let hop = (sample_rate / VALUES_PER_SECOND).round().max(1.) as usize;
        Self {
            scratch: vec![Complex::default(); fft.get_inplace_scratch_len()],
            fft,
            window,
            scale,
            num_channels: num_channels.max(1),
            // The first value is centred on the file's first frame.
            pending: vec![0.; FFT_SIZE / 2],
            spectrum: vec![Complex::default(); FFT_SIZE],
            previous: vec![0.; bins],
            current: vec![0.; bins],
            curve: OnsetCurve {
                sample_rate,
                hop,
                frames: 0,
                values: Vec::new(),
            },
        }
    }

    /// Add a block of planar samples (one slice per channel, all the same
    /// length).
    pub fn push(&mut self, block: &[&[f32]]) {
        let len = block.first().map_or(0, |channel| channel.len());
        let gain = 1. / self.num_channels as f32;
        self.curve.frames += len;
        self.pending.extend((0..len).map(|frame| {
            block
                .iter()
                .map(|channel| channel.get(frame).copied().unwrap_or(0.))
                .sum::<f32>()
                * gain
        }));
        self.analyse_pending();
    }

    /// The curve, with a value for every hop up to the last frame pushed.
    pub fn finish(mut self) -> OnsetCurve {
        // Silence after the end, for the last values' analyses to fill.
        self.pending.resize(self.pending.len() + FFT_SIZE / 2, 0.);
        self.analyse_pending();
        self.curve.values.shrink_to_fit();
        self.curve
    }

    fn analyse_pending(&mut self) {
        let hop = self.curve.hop;
        let mut start = 0;
        while self.pending.len() - start >= FFT_SIZE {
            let frame = &self.pending[start..start + FFT_SIZE];
            for ((bin, sample), weight) in self.spectrum.iter_mut().zip(frame).zip(&self.window) {
                *bin = Complex::new(sample * weight, 0.);
            }
            self.fft
                .process_with_scratch(&mut self.spectrum, &mut self.scratch);
            let mut flux = 0.;
            for ((current, previous), bin) in self
                .current
                .iter_mut()
                .zip(&self.previous)
                .zip(&self.spectrum)
            {
                *current = (COMPRESSION * bin.norm() * self.scale).ln_1p();
                flux += (*current - previous).max(0.);
            }
            std::mem::swap(&mut self.current, &mut self.previous);
            self.curve.values.push(flux);
            start += hop;
        }
        self.pending.drain(..start);
    }
}

/// Synthetic signals for the analysis tests.
#[cfg(test)]
pub(crate) mod signals {
    /// Uniform noise in -1..1, the same for a given `seed`.
    pub fn noise(len: usize, seed: u64) -> Vec<f32> {
        let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (0..len)
            .map(|_| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((state >> 40) as f32 / (1u64 << 23) as f32) - 1.
            })
            .collect()
    }

    /// A short, decaying noise burst (a hit) at each of `times` seconds,
    /// `seconds` long in all.
    pub fn hits(times: impl IntoIterator<Item = f64>, seconds: f64, rate: f64) -> Vec<f32> {
        let len = (seconds * rate).round() as usize;
        let mut out = vec![0.; len];
        let burst = noise((rate * 0.03) as usize, 7);
        let decay = (-1. / (rate as f32 * 0.008)).exp();
        for time in times {
            let start = (time * rate).round() as usize;
            let mut gain = 0.8;
            for (offset, sample) in burst.iter().enumerate() {
                if let Some(out) = out.get_mut(start + offset) {
                    *out += sample * gain;
                }
                gain *= decay;
            }
        }
        out
    }

    /// A hit on every beat at `bpm` for `seconds`, starting on the first
    /// frame.
    pub fn click_track(bpm: f64, seconds: f64, rate: f64) -> Vec<f32> {
        let beat = 60. / bpm;
        let count = (seconds / beat).ceil() as usize;
        hits(
            (0..count)
                .map(|index| index as f64 * beat)
                .filter(|time| *time < seconds),
            seconds,
            rate,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::signals::*;
    use super::*;

    const RATE: f64 = 48000.;

    fn curve(samples: &[f32]) -> OnsetCurve {
        OnsetCurve::from_buffer(&SampleBuffer::new(vec![samples.to_vec()], RATE))
    }

    /// One value per hop, the first on the first frame, the last at or
    /// before the last frame.
    #[test]
    fn a_value_per_hop() {
        let curve = curve(&vec![0.; 48000]);
        assert_eq!(curve.hop(), 480);
        assert_eq!(curve.frames(), 48000);
        assert_eq!(curve.len(), 48000 / 480 + 1);
        assert_eq!(curve.frame_of(3), 1440);
        assert!((curve.rate() - 100.).abs() < 1e-9);
        assert!(curve.values().iter().all(|value| *value == 0.), "silence");
    }

    /// Hits show as the curve's strongest values, at their frames.
    #[test]
    fn hits_are_its_peaks() {
        let times = [0.5, 1.25, 2.];
        let curve = curve(&hits(times, 3., RATE));
        let mut by_strength: Vec<usize> = (0..curve.len()).collect();
        by_strength.sort_by(|a, b| curve.values()[*b].total_cmp(&curve.values()[*a]));
        let mut strongest: Vec<f64> = by_strength[..3]
            .iter()
            .map(|index| curve.frame_of(*index) as f64 / RATE)
            .collect();
        strongest.sort_by(f64::total_cmp);
        for (found, time) in strongest.iter().zip(times) {
            assert!((found - time).abs() <= 0.015, "{found} for a hit at {time}");
        }
    }

    /// Pushed in blocks of any size, the curve is the same as at once.
    #[test]
    fn incremental_build_equals_one_shot() {
        let left = noise(30000, 1);
        let right = noise(30000, 2);
        let whole =
            OnsetCurve::from_buffer(&SampleBuffer::new(vec![left.clone(), right.clone()], RATE));
        let mut builder = OnsetBuilder::new(2, RATE);
        let mut at = 0;
        for size in [1, 700, 2047, 2048, 4099].into_iter().cycle() {
            if at >= left.len() {
                break;
            }
            let end = (at + size).min(left.len());
            builder.push(&[&left[at..end], &right[at..end]]);
            at = end;
        }
        assert_eq!(builder.finish(), whole);
    }

    #[test]
    fn bytes_round_trip() {
        let curve = curve(&click_track(120., 2., RATE));
        let bytes = curve.to_bytes();
        assert_eq!(OnsetCurve::from_bytes(&bytes), Some(curve));
        assert_eq!(OnsetCurve::from_bytes(&bytes[..bytes.len() - 1]), None);
        assert_eq!(OnsetCurve::from_bytes(b"TQPK"), None);
    }
}
