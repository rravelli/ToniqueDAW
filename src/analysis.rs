use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::get_probe;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use tonique_engine::peaks::WaveformPeaks;
use tonique_engine::sample::SampleBuffer;

use crate::waveform::{load_audio, load_cached_peaks};

#[derive(Debug)]
pub enum AudioInfoError {
    Io(std::io::Error),
    Symphonia(symphonia::core::errors::Error),
    MissingSampleRate,
    MissingChannels,
    NoDefaultTrack,
}

impl From<std::io::Error> for AudioInfoError {
    fn from(e: std::io::Error) -> Self {
        AudioInfoError::Io(e)
    }
}

impl From<symphonia::core::errors::Error> for AudioInfoError {
    fn from(e: symphonia::core::errors::Error) -> Self {
        AudioInfoError::Symphonia(e)
    }
}

/// Decoded content of a file, filled in by a background thread and shared
/// by every clip (and the engine source) using that file.
#[derive(Default)]
pub struct AudioData {
    /// Grows while the file decodes.
    peaks: RwLock<Arc<WaveformPeaks>>,
    /// Set once the whole file is decoded.
    samples: OnceLock<Arc<SampleBuffer>>,
}

impl AudioData {
    /// Already decoded data.
    #[cfg(test)]
    pub fn from_samples(buffer: SampleBuffer) -> Self {
        let data = Self::default();
        data.publish_peaks(WaveformPeaks::from_buffer(&buffer));
        data.finish(buffer);
        data
    }

    pub fn peaks(&self) -> Arc<WaveformPeaks> {
        self.peaks.read().map(|p| p.clone()).unwrap_or_default()
    }

    /// Decoded samples at the file's rate, once decoding has finished.
    pub fn samples(&self) -> Option<&Arc<SampleBuffer>> {
        self.samples.get()
    }

    pub fn is_ready(&self) -> bool {
        self.samples.get().is_some()
    }

    pub fn publish_peaks(&self, peaks: WaveformPeaks) {
        if let Ok(mut current) = self.peaks.write() {
            *current = Arc::new(peaks);
        }
    }

    pub fn finish(&self, samples: SampleBuffer) {
        let _ = self.samples.set(Arc::new(samples));
    }
}

impl std::fmt::Debug for AudioData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioData")
            .field("peak_frames", &self.peaks().frames())
            .field("ready", &self.is_ready())
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct AudioInfo {
    pub name: String,
    pub duration: Option<Duration>,
    pub data: Arc<AudioData>,
    pub sample_rate: u32,
    pub channels: u16,
    pub bit_depth: Option<u32>,
    pub num_samples: Option<u64>,
    pub path: PathBuf,
}

impl AudioInfo {
    /// Length of the file in frames, at its own sample rate.
    pub fn total_frames(&self) -> Option<f64> {
        match (self.data.samples(), self.num_samples, self.duration) {
            (Some(samples), _, _) => Some(samples.len() as f64),
            (None, Some(n), _) => Some(n as f64),
            (None, None, Some(d)) => Some(d.as_secs_f64() * self.sample_rate as f64),
            _ => None,
        }
    }
}

pub fn get_audio_info<P: AsRef<Path>>(path: P) -> Result<AudioInfo, AudioInfoError> {
    let name = path
        .as_ref()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let file = File::open(&path)?;

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let hint = Hint::new();
    let probed =
        get_probe().format(&hint, mss, &Default::default(), &MetadataOptions::default())?;
    let format = probed.format;

    let track = format
        .default_track()
        .ok_or(AudioInfoError::NoDefaultTrack)?;

    let codec_params = &track.codec_params;

    let sample_rate = codec_params
        .sample_rate
        .ok_or(AudioInfoError::MissingSampleRate)?;

    let channels = codec_params
        .channels
        .ok_or(AudioInfoError::MissingChannels)?
        .count() as u16;

    let duration = codec_params
        .n_frames
        .map(|frames| Duration::from_secs_f64(frames as f64 / sample_rate as f64));

    let data = Arc::new(AudioData::default());
    if let Some(peaks) = load_cached_peaks(path.as_ref()) {
        data.publish_peaks(peaks);
    }
    let data_ref = data.clone();
    let p = path.as_ref().to_path_buf();

    std::thread::spawn(move || {
        if let Err(e) = load_audio(&p, &data) {
            eprintln!("Failed to load {}: {e}", p.display());
        }
    });

    Ok(AudioInfo {
        name,
        duration,
        sample_rate,
        channels,
        bit_depth: codec_params.bits_per_sample,
        num_samples: codec_params.n_frames,
        path: path.as_ref().to_path_buf(),
        data: data_ref,
    })
}
