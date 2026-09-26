use tonique_engine::{
    device::{DeviceError, OutputDevice},
    engine::{Engine, EngineConfig},
};

/// Start the engine on the default output device. Keep the returned stream
/// alive to keep audio running.
pub fn start_audio() -> Result<(cpal::Stream, Engine), DeviceError> {
    let device = OutputDevice::default_output()?;
    let (engine, processor) = Engine::new(EngineConfig {
        sample_rate: device.sample_rate(),
        max_block: 256,
        worker_threads: 2,
        ..Default::default()
    });
    let stream = device.start(processor)?;
    Ok((stream, engine))
}
