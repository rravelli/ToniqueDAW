//! Immutable sample data shared by `Arc`, plus WAV I/O.
//!
//! Sample data is converted to the engine rate once, at load time, off the
//! RT thread; playback nodes only ever read pre-converted samples.

use std::path::Path;
use std::sync::Arc;

use crate::audio::AudioBlock;
use crate::resample::resample_offline;

/// Planar, read-only audio data. N clips referencing the same file share
/// one `Arc<SampleBuffer>`: no copies, just refcount bumps.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleBuffer {
    channels: Vec<Vec<f32>>,
    sample_rate: f64,
}

impl SampleBuffer {
    pub fn new(channels: Vec<Vec<f32>>, sample_rate: f64) -> Self {
        assert!(!channels.is_empty());
        let len = channels[0].len();
        assert!(channels.iter().all(|c| c.len() == len), "channels must have equal length");
        Self { channels, sample_rate }
    }

    pub fn from_interleaved(data: &[f32], num_channels: usize, sample_rate: f64) -> Self {
        let channels = (0..num_channels).map(|c| data.iter().skip(c).step_by(num_channels).copied().collect()).collect();
        Self::new(channels, sample_rate)
    }

    pub fn len(&self) -> usize {
        self.channels[0].len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn num_channels(&self) -> usize {
        self.channels.len()
    }

    pub fn channel(&self, ch: usize) -> &[f32] {
        &self.channels[ch]
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// High-quality conversion to another rate (windowed sinc).
    pub fn resampled(&self, target_rate: f64) -> Self {
        if target_rate == self.sample_rate {
            return self.clone();
        }
        let channels = self.channels.iter().map(|c| resample_offline(c, self.sample_rate, target_rate)).collect();
        Self::new(channels, target_rate)
    }

    /// Load a WAV file, converting to `target_rate` if needed.
    pub fn load_wav(path: impl AsRef<Path>, target_rate: f64) -> Result<Arc<Self>, hound::Error> {
        let mut reader = hound::WavReader::open(path)?;
        let spec = reader.spec();
        let data: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
            hound::SampleFormat::Int => {
                let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
                reader.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
            }
        };
        let buf = Self::from_interleaved(&data, spec.channels as usize, spec.sample_rate as f64);
        Ok(Arc::new(buf.resampled(target_rate)))
    }
}

/// Incrementally writes planar blocks to a 32-bit float WAV file.
pub struct WavWriter {
    inner: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    channels: usize,
}

impl WavWriter {
    pub fn create(path: impl AsRef<Path>, channels: usize, sample_rate: f64) -> Result<Self, hound::Error> {
        let spec = hound::WavSpec {
            channels: channels as u16,
            sample_rate: sample_rate.round() as u32,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        Ok(Self { inner: hound::WavWriter::create(path, spec)?, channels })
    }

    pub fn write_block(&mut self, block: &AudioBlock) -> Result<(), hound::Error> {
        for i in 0..block.len() {
            for ch in 0..self.channels {
                let s = if block.channels() == 0 { 0.0 } else { block.channel(ch.min(block.channels() - 1))[i] };
                self.inner.write_sample(s)?;
            }
        }
        Ok(())
    }

    pub fn write_interleaved(&mut self, data: &[f32]) -> Result<(), hound::Error> {
        data.iter().try_for_each(|s| self.inner.write_sample(*s))
    }

    pub fn finalize(self) -> Result<(), hound::Error> {
        self.inner.finalize()
    }
}
