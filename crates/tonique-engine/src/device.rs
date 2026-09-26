//! Real-time output through cpal (enable the `device` feature).

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::engine::AudioProcessor;
use crate::rt;

pub type DeviceError = Box<dyn std::error::Error + Send + Sync>;

/// The system's default output device and its preferred configuration.
pub struct OutputDevice {
    device: cpal::Device,
    config: cpal::StreamConfig,
}

impl OutputDevice {
    pub fn default_output() -> Result<Self, DeviceError> {
        let device = cpal::default_host().default_output_device().ok_or("no default output device")?;
        let config = device.default_output_config()?.config();
        Ok(Self { device, config })
    }

    /// Configure the engine with this rate so no device-rate resampling is needed.
    pub fn sample_rate(&self) -> f64 {
        self.config.sample_rate as f64
    }

    pub fn channels(&self) -> usize {
        self.config.channels as usize
    }

    /// Start streaming. Keep the returned stream alive to keep playing.
    pub fn start(self, mut processor: AudioProcessor) -> Result<cpal::Stream, DeviceError> {
        if processor.config().sample_rate != self.sample_rate() {
            return Err("engine sample rate differs from the device's".into());
        }
        let channels = self.channels();
        let mut first = true;
        let stream = self.device.build_output_stream::<f32, _, _>(
            self.config,
            move |data, _info| {
                if first {
                    first = false;
                    rt::enable_flush_denormals();
                    // Best effort: needs rtkit or an rtprio limit.
                    let _ = rt::set_realtime_priority(70);
                }
                processor.process_interleaved(data, channels);
            },
            |err| eprintln!("audio stream error: {err}"),
            None,
        )?;
        stream.play()?;
        Ok(stream)
    }
}
