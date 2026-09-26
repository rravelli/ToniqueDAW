//! Real-time output through cpal (enable the `device` feature).

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::engine::AudioProcessor;
use crate::rt;

pub type DeviceError = Box<dyn std::error::Error + Send + Sync>;

/// Sample rates offered when a device supports a continuous range.
const COMMON_RATES: [u32; 8] = [22050, 32000, 44100, 48000, 88200, 96000, 176400, 192000];

/// An output device of the default host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Stable identifier, to find the device again (see [`DeviceOptions`]).
    pub id: String,
    /// Human-readable name.
    pub name: String,
}

/// Output devices of the default host. Can be slow (it probes hardware):
/// don't call it every frame.
pub fn output_devices() -> Vec<DeviceInfo> {
    let Ok(devices) = cpal::default_host().output_devices() else { return Vec::new() };
    devices.filter_map(|d| Some(DeviceInfo { id: d.id().ok()?.to_string(), name: device_name(&d) })).collect()
}

fn device_name(device: &cpal::Device) -> String {
    device.description().map_or_else(|_| "Unknown device".into(), |d| d.name().to_string())
}

/// Which device to open and how. `None` fields use the device's defaults;
/// unsupported values fall back to them too.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceOptions {
    /// A [`DeviceInfo::id`]; `None` (or a device that's gone) means the
    /// system default.
    pub device_id: Option<String>,
    pub sample_rate: Option<u32>,
    /// Frames per device callback.
    pub buffer_frames: Option<u32>,
}

/// An output device and the configuration it will be opened with.
pub struct OutputDevice {
    device: cpal::Device,
    config: cpal::StreamConfig,
}

impl OutputDevice {
    /// The system's default output device and its preferred configuration.
    pub fn default_output() -> Result<Self, DeviceError> {
        Self::open(&DeviceOptions::default())
    }

    pub fn open(options: &DeviceOptions) -> Result<Self, DeviceError> {
        let host = cpal::default_host();
        let wanted = options.device_id.as_ref().and_then(|id| {
            host.output_devices().ok()?.find(|d| d.id().is_ok_and(|d| d.to_string() == *id))
        });
        let device = wanted.or_else(|| host.default_output_device()).ok_or("no output device available")?;
        let mut config = device.default_output_config()?.config();
        let mut out = Self { device, config };
        if let Some(rate) = options.sample_rate.filter(|r| out.supported_sample_rates().contains(r)) {
            config.sample_rate = rate;
        }
        if let Some(frames) = options.buffer_frames
            && out.buffer_size_range().is_none_or(|(min, max)| (min..=max).contains(&frames))
        {
            config.buffer_size = cpal::BufferSize::Fixed(frames);
        }
        out.config = config;
        Ok(out)
    }

    pub fn name(&self) -> String {
        device_name(&self.device)
    }

    /// Sample rates the device supports with its channel count, lowest first.
    pub fn supported_sample_rates(&self) -> Vec<u32> {
        let Ok(configs) = self.device.supported_output_configs() else { return Vec::new() };
        let mut rates: Vec<u32> = configs
            .filter(|c| c.channels() == self.config.channels)
            .flat_map(|c| {
                let (min, max) = (c.min_sample_rate(), c.max_sample_rate());
                // A fixed rate is listed as is; ranges (ALSA's can be
                // 1 Hz..384 kHz) offer the usual rates within them.
                COMMON_RATES.into_iter().filter(move |r| (min..=max).contains(r)).chain((min == max).then_some(min))
            })
            .collect();
        rates.sort_unstable();
        rates.dedup();
        rates
    }

    /// Smallest and largest callback size the device accepts, if it says.
    pub fn buffer_size_range(&self) -> Option<(u32, u32)> {
        let configs = self.device.supported_output_configs().ok()?;
        configs.filter(|c| c.channels() == self.config.channels).find_map(|c| match *c.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => Some((min, max)),
            cpal::SupportedBufferSize::Unknown => None,
        })
    }

    /// Frames per callback, if fixed (otherwise the device decides).
    pub fn buffer_frames(&self) -> Option<u32> {
        match self.config.buffer_size {
            cpal::BufferSize::Fixed(frames) => Some(frames),
            cpal::BufferSize::Default => None,
        }
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
