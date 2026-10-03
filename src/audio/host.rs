use std::{
    sync::mpsc::{self, Receiver},
    thread,
};

use crate::config::settings::Settings;
use tonique_engine::{
    device::{DeviceError, OutputDevice, OutputStream},
    engine::Engine,
};

/// The running audio output. Dropping it stops the stream.
pub struct AudioHost {
    stream: OutputStream,
    /// What was actually opened (settings the device doesn't support fall
    /// back to its defaults).
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: usize,
    pub buffer_frames: Option<u32>,
}

impl AudioHost {
    /// Whether the output stopped for good (e.g. the device was unplugged).
    pub fn is_lost(&self) -> bool {
        self.stream.is_lost()
    }
}

/// An engine with no output, for when no device can be opened. Commands
/// queue up unheard until the project moves to a working engine.
pub fn engine_without_audio(settings: &Settings) -> Engine {
    const SAMPLE_RATE: f64 = 48_000.;
    let (engine, _processor) = Engine::new(settings.engine_config(SAMPLE_RATE));
    engine
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
        stream,
        device_name,
        sample_rate,
        channels,
        buffer_frames,
    };
    Ok((host, engine))
}

/// Check on another thread whether an output can be opened with
/// `settings`: listing devices can block for a while on some systems.
pub fn probe_output(settings: &Settings) -> Receiver<bool> {
    let options = settings.device_options();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        // The receiver may be gone if audio came back meanwhile.
        let _ = tx.send(OutputDevice::open(&options).is_ok());
    });
    rx
}
