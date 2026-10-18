use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::get_probe;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use tonique_engine::peaks::WaveformPeaks;
use tonique_engine::sample::SampleBuffer;

use crate::waveform::{load_audio, load_cached_peaks};

#[derive(Debug)]
pub enum AudioInfoError {
    Io,
    Symphonia,
    MissingSampleRate,
    MissingChannels,
    NoDefaultTrack,
}

impl From<std::io::Error> for AudioInfoError {
    fn from(_: std::io::Error) -> Self {
        AudioInfoError::Io
    }
}

impl From<symphonia::core::errors::Error> for AudioInfoError {
    fn from(_: symphonia::core::errors::Error) -> Self {
        AudioInfoError::Symphonia
    }
}

/// Decoded content of a file, filled in by background threads and shared
/// by every clip (and the engine source) using that file.
///
/// Nothing is decoded up front: [`AudioInfo::load_peaks`] decodes only to
/// draw the waveform (browsing), [`AudioInfo::load_samples`] keeps the
/// samples too (the file is used by a clip).
#[derive(Default)]
pub struct AudioData {
    /// Grows while the file decodes.
    peaks: RwLock<Arc<WaveformPeaks>>,
    /// Set once `peaks` covers the whole file.
    peaks_complete: AtomicBool,
    /// Set once the whole file is decoded.
    samples: OnceLock<Arc<SampleBuffer>>,
    peaks_requested: AtomicBool,
    samples_requested: AtomicBool,
}

impl AudioData {
    /// Already decoded data.
    #[cfg(test)]
    pub fn from_samples(buffer: SampleBuffer) -> Self {
        let data = Self::default();
        data.complete_peaks(WaveformPeaks::from_buffer(&buffer));
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

    pub fn peaks_complete(&self) -> bool {
        self.peaks_complete.load(Ordering::Acquire)
    }

    /// Whether a decode that keeps the samples has been started.
    pub fn samples_requested(&self) -> bool {
        self.samples_requested.load(Ordering::Acquire)
    }

    /// Replace the peaks, unless they'd cover less of the file: two decodes
    /// may publish concurrently (browsing, then using the file in a clip).
    pub fn publish_peaks(&self, peaks: WaveformPeaks) {
        if let Ok(mut current) = self.peaks.write()
            && !self.peaks_complete()
            && peaks.frames() >= current.frames()
        {
            *current = Arc::new(peaks);
        }
    }

    pub fn complete_peaks(&self, peaks: WaveformPeaks) {
        self.publish_peaks(peaks);
        self.peaks_complete.store(true, Ordering::Release);
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
    /// A decoded, silent file of `seconds`, for tests. At 120 bpm that's
    /// `2 * seconds` beats.
    #[cfg(test)]
    pub fn silent(seconds: f32) -> Self {
        Self {
            name: "test.wav".into(),
            duration: Some(Duration::from_secs_f32(seconds)),
            data: Arc::new(AudioData::from_samples(SampleBuffer::new(
                vec![vec![0.; 480], vec![0.; 480]],
                48000.,
            ))),
            sample_rate: 48000,
            channels: 2,
            bit_depth: None,
            num_samples: None,
            path: std::path::PathBuf::from(format!("test-{seconds}.wav")),
        }
    }

    /// Decode the file in the background to draw its waveform, without
    /// keeping its samples. Stops early once nothing uses the file anymore.
    pub fn load_peaks(&self) {
        let data = &self.data;
        if data.peaks_complete()
            || data.samples_requested()
            || data.peaks_requested.swap(true, Ordering::AcqRel)
        {
            return;
        }
        self.spawn_decode(false);
    }

    /// Decode the whole file in the background and keep its samples.
    pub fn load_samples(&self) {
        if !self.data.samples_requested.swap(true, Ordering::AcqRel) {
            self.spawn_decode(true);
        }
    }

    fn spawn_decode(&self, keep_samples: bool) {
        let data = Arc::downgrade(&self.data);
        let path = self.path.clone();
        std::thread::spawn(move || {
            if let Err(e) = load_audio(&path, &data, keep_samples) {
                eprintln!("Failed to load {}: {e}", path.display());
            }
        });
    }

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

pub fn read_audio_info<P: AsRef<Path>>(path: P) -> Result<AudioInfo, AudioInfoError> {
    let name = path
        .as_ref()
        .file_name()
        .unwrap_or(path.as_ref().as_os_str())
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
        data.complete_peaks(peaks);
    }

    Ok(AudioInfo {
        name,
        duration,
        sample_rate,
        channels,
        bit_depth: codec_params.bits_per_sample,
        num_samples: codec_params.n_frames,
        path: path.as_ref().to_path_buf(),
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::AUDIO_ANALYSIS_CACHE;
    use std::time::Instant;
    use tonique_engine::sample::WavWriter;

    /// A fresh 1 s stereo WAV (fresh so no cached peaks exist for it).
    fn write_wav(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tonique-load-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{name}.wav"));
        let mut wav = WavWriter::create(&path, 2, 48000.).unwrap();
        wav.write_interleaved(&vec![0.25; 2 * 48000]).unwrap();
        wav.finalize().unwrap();
        path
    }

    fn wait_for(what: &str, done: impl Fn() -> bool) {
        let start = Instant::now();
        while !done() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "timed out: {what}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Browsing a file builds its waveform without keeping its samples;
    /// only using it decodes them.
    #[test]
    fn samples_are_only_decoded_on_request() {
        let info = read_audio_info(write_wav("lazy")).unwrap();
        info.load_peaks();
        wait_for("peaks", || info.data.peaks_complete());
        assert_eq!(info.data.peaks().frames(), 48000);
        assert!(info.data.samples().is_none());

        info.load_samples();
        wait_for("samples", || info.data.is_ready());
        let samples = info.data.samples().unwrap();
        assert_eq!(samples.len(), 48000);
        assert_eq!(samples.channel(1)[100], 0.25);
    }

    /// The cache lets go of files nothing else holds.
    #[test]
    fn cache_forgets_unused_files() {
        let (a, b) = (write_wav("a"), write_wav("b"));
        let first = AUDIO_ANALYSIS_CACHE.get_or_analyze(a.clone()).unwrap();
        let again = AUDIO_ANALYSIS_CACHE.get_or_analyze(a.clone()).unwrap();
        assert!(Arc::ptr_eq(&first.data, &again.data));

        // A running decode doesn't count as a use.
        first.load_peaks();
        let weak = Arc::downgrade(&first.data);
        drop((first, again));
        wait_for("forgotten", || {
            let _other = AUDIO_ANALYSIS_CACHE.get_or_analyze(b.clone());
            weak.upgrade().is_none()
        });
    }
}
