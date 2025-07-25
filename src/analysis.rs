use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::get_probe;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::waveform::load_audio;

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

#[derive(Clone, Debug)]
pub struct AudioInfo {
    pub name: String,
    pub duration: Option<Duration>,
    pub data: Arc<Mutex<(Vec<f32>, Vec<f32>)>>,
    pub ready: Arc<Mutex<bool>>,
    pub sample_rate: u32,
    pub channels: u16,
    pub is_stereo: bool,
    pub codec: String,
    pub bit_depth: Option<u32>,
    pub num_samples: Option<u64>,
    pub path: PathBuf,
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

    let is_stereo = channels == 2;

    let duration = codec_params
        .n_frames
        .map(|frames| Duration::from_secs_f64(frames as f64 / sample_rate as f64));

    let data = Arc::new(Mutex::new((Vec::new(), Vec::new())));
    let data_ref = data.clone();
    let analyzed = Arc::new(Mutex::new(false));
    let ready_clone = analyzed.clone();
    let p = path.as_ref().to_string_lossy().to_string();

    std::thread::spawn(move || {
        let _ = load_audio(p, data, analyzed);
    });

    Ok(AudioInfo {
        name,
        duration,
        sample_rate,
        channels,
        is_stereo,
        codec: format!("{:?}", codec_params.codec),
        bit_depth: codec_params.bits_per_sample,
        num_samples: codec_params.n_frames,
        ready: ready_clone,
        path: path.as_ref().to_path_buf(),
        data: data_ref,
    })
}
