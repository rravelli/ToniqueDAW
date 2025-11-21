use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::get_probe;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use crate::waveform::load_audio;

#[derive(Clone, Debug)]
pub struct AudioInfo {
    pub name: String,
    pub duration: Option<Duration>,
    pub data: Arc<RwLock<(Vec<f32>, Vec<f32>)>>,
    pub ready: Arc<RwLock<bool>>,
    pub sample_rate: u32,
    pub channels: u16,
    pub bit_depth: Option<u32>,
    pub num_samples: Option<u64>,
    pub path: PathBuf,
}

pub fn get_audio_info<P: AsRef<Path>>(path: P) -> Result<AudioInfo, String> {
    let name = path
        .as_ref()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let file = File::open(&path).map_err(|e| e.to_string())?;

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let hint = Hint::new();
    let probed = get_probe()
        .format(&hint, mss, &Default::default(), &MetadataOptions::default())
        .map_err(|e| e.to_string())?;
    let format = probed.format;

    let track = format
        .default_track()
        .ok_or("No default track".to_string())?;

    let codec_params = &track.codec_params;

    let sample_rate = codec_params.sample_rate.ok_or("Missing sample rate")?;

    let channels = codec_params.channels.ok_or("Missing channels")?.count() as u16;

    let duration = codec_params
        .n_frames
        .map(|frames| Duration::from_secs_f64(frames as f64 / sample_rate as f64));

    let data = Arc::new(RwLock::new((Vec::new(), Vec::new())));
    let data_ref = data.clone();
    let analyzed = Arc::new(RwLock::new(false));
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
        bit_depth: codec_params.bits_per_sample,
        num_samples: codec_params.n_frames,
        ready: ready_clone,
        path: path.as_ref().to_path_buf(),
        data: data_ref,
    })
}
