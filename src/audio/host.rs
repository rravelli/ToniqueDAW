use crate::config::settings::Settings;
use tonique_engine::{
    device::{DeviceError, OutputDevice},
    engine::Engine,
};

/// The running audio output. Dropping it stops the stream.
pub struct AudioHost {
    _stream: cpal::Stream,
    /// What was actually opened (settings the device doesn't support fall
    /// back to its defaults).
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: usize,
    pub buffer_frames: Option<u32>,
}

/// Start the engine on the output device chosen in `settings`.
pub fn start_audio(settings: &Settings) -> Result<(AudioHost, Engine), DeviceError> {
    let device = OutputDevice::open(&settings.device_options())?;
    let (engine, processor) = Engine::new(settings.engine_config(device.sample_rate()));
    let (device_name, sample_rate, channels, buffer_frames) = (
        device.name(),
        device.sample_rate() as u32,
        device.channels(),
        device.buffer_frames(),
    );
    let stream = device.start(processor)?;
    let host = AudioHost {
        _stream: stream,
        device_name,
        sample_rate,
        channels,
        buffer_frames,
    };
    Ok((host, engine))
}
