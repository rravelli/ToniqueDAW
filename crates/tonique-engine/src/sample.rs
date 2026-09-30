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
        assert!(
            channels.iter().all(|c| c.len() == len),
            "channels must have equal length"
        );
        Self {
            channels,
            sample_rate,
        }
    }

    pub fn from_interleaved(data: &[f32], num_channels: usize, sample_rate: f64) -> Self {
        let channels = (0..num_channels)
            .map(|c| data.iter().skip(c).step_by(num_channels).copied().collect())
            .collect();
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
        // Channels are independent: convert them in parallel.
        let channels = std::thread::scope(|scope| {
            let jobs: Vec<_> = self
                .channels
                .iter()
                .map(|c| scope.spawn(move || resample_offline(c, self.sample_rate, target_rate)))
                .collect();
            jobs.into_iter()
                .map(|j| j.join().expect("resampling thread panicked"))
                .collect()
        });
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
                reader
                    .samples::<i32>()
                    .map(|s| s.map(|v| v as f32 * scale))
                    .collect::<Result<_, _>>()?
            }
        };
        let buf = Self::from_interleaved(&data, spec.channels as usize, spec.sample_rate as f64);
        Ok(Arc::new(buf.resampled(target_rate)))
    }
}

/// Sample format of a written WAV file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WavFormat {
    Int16,
    #[default]
    Int24,
    Float32,
}

impl WavFormat {
    fn spec(self, channels: usize, sample_rate: f64) -> hound::WavSpec {
        let (bits_per_sample, sample_format) = match self {
            WavFormat::Int16 => (16, hound::SampleFormat::Int),
            WavFormat::Int24 => (24, hound::SampleFormat::Int),
            WavFormat::Float32 => (32, hound::SampleFormat::Float),
        };
        hound::WavSpec {
            channels: channels as u16,
            sample_rate: sample_rate.round() as u32,
            bits_per_sample,
            sample_format,
        }
    }
}

/// Incrementally writes audio to a WAV file (32-bit float unless created
/// with [`WavWriter::create_as`]). Integer formats clip outside -1..1.
pub struct WavWriter {
    inner: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    channels: usize,
    format: WavFormat,
}

impl WavWriter {
    pub fn create(
        path: impl AsRef<Path>,
        channels: usize,
        sample_rate: f64,
    ) -> Result<Self, hound::Error> {
        Self::create_as(path, channels, sample_rate, WavFormat::Float32)
    }

    pub fn create_as(
        path: impl AsRef<Path>,
        channels: usize,
        sample_rate: f64,
        format: WavFormat,
    ) -> Result<Self, hound::Error> {
        Ok(Self {
            inner: hound::WavWriter::create(path, format.spec(channels, sample_rate))?,
            channels,
            format,
        })
    }

    fn write_sample(&mut self, s: f32) -> Result<(), hound::Error> {
        let int = |max: f32| (s.clamp(-1.0, 1.0) * max).round() as i32;
        match self.format {
            WavFormat::Int16 => self.inner.write_sample(int(i16::MAX as f32) as i16),
            WavFormat::Int24 => self.inner.write_sample(int(8_388_607.0)),
            WavFormat::Float32 => self.inner.write_sample(s),
        }
    }

    pub fn write_block(&mut self, block: &AudioBlock) -> Result<(), hound::Error> {
        for i in 0..block.len() {
            for ch in 0..self.channels {
                let s = if block.channels() == 0 {
                    0.0
                } else {
                    block.channel(ch.min(block.channels() - 1))[i]
                };
                self.write_sample(s)?;
            }
        }
        Ok(())
    }

    pub fn write_interleaved(&mut self, data: &[f32]) -> Result<(), hound::Error> {
        data.iter().try_for_each(|s| self.write_sample(*s))
    }

    pub fn finalize(self) -> Result<(), hound::Error> {
        self.inner.finalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Integer formats scale full scale to their largest value and clip.
    #[test]
    fn writes_integer_and_float_wavs() {
        let dir = std::env::temp_dir().join(format!("tonique-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let data = [0.0, 0.5, -1.0, 2.0];
        for (format, expected) in [
            (WavFormat::Int16, vec![0, 16384, -32767, 32767]),
            (WavFormat::Int24, vec![0, 4_194_304, -8_388_607, 8_388_607]),
        ] {
            let path = dir.join(format!("{format:?}.wav"));
            let mut wav = WavWriter::create_as(&path, 2, 44100., format).unwrap();
            wav.write_interleaved(&data).unwrap();
            wav.finalize().unwrap();
            let mut reader = hound::WavReader::open(&path).unwrap();
            assert_eq!(reader.spec().sample_rate, 44100);
            let read: Vec<i32> = reader.samples::<i32>().map(Result::unwrap).collect();
            assert_eq!(read, expected, "{format:?}");
        }
        let path = dir.join("float.wav");
        let mut wav = WavWriter::create(&path, 2, 48000.).unwrap();
        wav.write_interleaved(&data).unwrap();
        wav.finalize().unwrap();
        let mut reader = hound::WavReader::open(&path).unwrap();
        let read: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).collect();
        assert_eq!(read, data);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
