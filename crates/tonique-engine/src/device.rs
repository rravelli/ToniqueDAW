//! Real-time output and input through cpal (enable the `device` feature).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, SampleFormat, SizedSample, SupportedStreamConfigRange};

use crate::engine::AudioProcessor;
use crate::input::InputWriter;
use crate::rt;

pub type DeviceError = Box<dyn std::error::Error + Send + Sync>;

/// Sample rates offered when a device supports a continuous range.
const COMMON_RATES: [u32; 8] = [22050, 32000, 44100, 48000, 88200, 96000, 176400, 192000];

/// Frames per callback through a sound server when none is asked for: left
/// to itself, PulseAudio buffers about two seconds.
const SERVER_BUFFER_FRAMES: u32 = 512;

/// A device of the default host.
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
    let Ok(devices) = cpal::default_host().output_devices() else {
        return Vec::new();
    };
    device_infos(devices)
}

/// Input devices of the default host. Can be slow (it probes hardware):
/// don't call it every frame. Doesn't start capturing from any of them.
pub fn input_devices() -> Vec<DeviceInfo> {
    let Ok(devices) = cpal::default_host().input_devices() else {
        return Vec::new();
    };
    device_infos(devices)
}

fn device_infos(devices: impl Iterator<Item = cpal::Device>) -> Vec<DeviceInfo> {
    devices
        .filter_map(|d| {
            Some(DeviceInfo {
                id: d.id().ok()?.to_string(),
                name: device_name(&d),
            })
        })
        .collect()
}

fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map_or_else(|_| "Unknown device".into(), |d| d.name().to_string())
}

/// Whether the device is reached through a sound server, which picks a
/// large buffer when not given one.
fn on_sound_server(device: &cpal::Device) -> bool {
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd"
    ))]
    return device
        .id()
        .is_ok_and(|id| id.host() == cpal::HostId::PulseAudio);
    #[allow(unreachable_code)]
    false
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
            host.output_devices()
                .ok()?
                .find(|d| d.id().is_ok_and(|d| d.to_string() == *id))
        });
        let device = wanted
            .or_else(|| host.default_output_device())
            .ok_or("no output device available")?;
        let mut config = device.default_output_config()?.config();
        let mut out = Self { device, config };
        if let Some(rate) = options
            .sample_rate
            .filter(|r| out.supported_sample_rates().contains(r))
        {
            config.sample_rate = rate;
        }
        let frames = options
            .buffer_frames
            .or_else(|| on_sound_server(&out.device).then_some(SERVER_BUFFER_FRAMES));
        if let Some(frames) = frames
            && out
                .buffer_size_range()
                .is_none_or(|(min, max)| (min..=max).contains(&frames))
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
        let Ok(configs) = self.device.supported_output_configs() else {
            return Vec::new();
        };
        let mut rates: Vec<u32> = configs
            .filter(|c| c.channels() == self.config.channels)
            .flat_map(|c| {
                let (min, max) = (c.min_sample_rate(), c.max_sample_rate());
                // A fixed rate is listed as is; ranges (ALSA's can be
                // 1 Hz..384 kHz) offer the usual rates within them.
                COMMON_RATES
                    .into_iter()
                    .filter(move |r| (min..=max).contains(r))
                    .chain((min == max).then_some(min))
            })
            .collect();
        rates.sort_unstable();
        rates.dedup();
        rates
    }

    /// Smallest and largest callback size the device accepts, if it says.
    pub fn buffer_size_range(&self) -> Option<(u32, u32)> {
        let configs = self.device.supported_output_configs().ok()?;
        configs
            .filter(|c| c.channels() == self.config.channels)
            .find_map(|c| match *c.buffer_size() {
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
    pub fn start(self, mut processor: AudioProcessor) -> Result<OutputStream, DeviceError> {
        if processor.config().sample_rate != self.sample_rate() {
            return Err("engine sample rate differs from the device's".into());
        }
        let channels = self.channels();
        let rate = self.sample_rate();
        let mut first = true;
        let lost = Arc::new(AtomicBool::new(false));
        let stream = self.device.build_output_stream::<f32, _, _>(
            self.config,
            move |data, info| {
                if first {
                    first = false;
                    rt::enable_flush_denormals();
                    // Best effort: needs rtkit or an rtprio limit.
                    let _ = rt::set_realtime_priority(70);
                }
                let time = info.timestamp();
                let latency = time.playback.saturating_duration_since(time.callback);
                processor.set_output_latency_frames(duration_frames(latency, rate));
                processor.process_interleaved(data, channels);
            },
            on_stream_error(&lost),
            None,
        )?;
        stream.play()?;
        Ok(OutputStream {
            _stream: stream,
            lost,
        })
    }
}

/// A playing output stream. Dropping it stops the stream.
pub struct OutputStream {
    _stream: cpal::Stream,
    lost: Arc<AtomicBool>,
}

impl OutputStream {
    /// Whether the stream stopped for good (e.g. the device was unplugged):
    /// open a device again to keep playing.
    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }
}

/// An input device and the configuration it will be opened with.
pub struct InputDevice {
    device: cpal::Device,
    config: cpal::StreamConfig,
    format: SampleFormat,
}

impl InputDevice {
    /// A device (a [`DeviceInfo::id`], or `None` for the system default) at
    /// the engine's rate. Doesn't start capturing yet. Unlike an output, a
    /// device that's gone is an error rather than the default: recording
    /// from another microphone than the one chosen would go unnoticed.
    pub fn open(
        device_id: Option<&str>,
        sample_rate: f64,
        buffer_frames: Option<u32>,
    ) -> Result<Self, DeviceError> {
        let host = cpal::default_host();
        let device = match device_id {
            Some(id) => host
                .input_devices()?
                .find(|d| d.id().is_ok_and(|d| d.to_string() == id))
                .ok_or("input device not found")?,
            None => host
                .default_input_device()
                .ok_or("no input device available")?,
        };
        let rate = sample_rate.round() as u32;
        let default = device.default_input_config()?;
        let supported: Vec<_> = device.supported_input_configs()?.collect();
        let picked = pick_input_config(
            &supported,
            default.channels(),
            default.sample_format(),
            rate,
        );
        let (channels, format, buffer_range) = match picked {
            Some(range) => (
                range.channels(),
                range.sample_format(),
                buffer_range(range.buffer_size()),
            ),
            // Not every host lists its default configuration.
            None if default.sample_rate() == rate && can_convert(default.sample_format()) => (
                default.channels(),
                default.sample_format(),
                buffer_range(default.buffer_size()),
            ),
            None => return Err(format!("input device can't run at {rate} Hz").into()),
        };
        let frames = buffer_frames
            .or_else(|| on_sound_server(&device).then_some(SERVER_BUFFER_FRAMES))
            .filter(|f| buffer_range.is_none_or(|(min, max)| (min..=max).contains(f)));
        let config = cpal::StreamConfig {
            channels,
            sample_rate: rate,
            buffer_size: frames.map_or(cpal::BufferSize::Default, cpal::BufferSize::Fixed),
        };
        Ok(Self {
            device,
            config,
            format,
        })
    }

    pub fn name(&self) -> String {
        device_name(&self.device)
    }

    pub fn sample_rate(&self) -> f64 {
        self.config.sample_rate as f64
    }

    pub fn channels(&self) -> usize {
        self.config.channels as usize
    }

    /// Frames per callback, if fixed (otherwise the device decides).
    pub fn buffer_frames(&self) -> Option<u32> {
        match self.config.buffer_size {
            cpal::BufferSize::Fixed(frames) => Some(frames),
            cpal::BufferSize::Default => None,
        }
    }

    /// Start capturing into `writer`, which must have this device's channel
    /// count. Keep the returned stream alive to keep capturing.
    pub fn start(self, writer: InputWriter) -> Result<InputStream, DeviceError> {
        if writer.channels() != self.channels() {
            return Err("input feed's channel count differs from the device's".into());
        }
        match self.format {
            SampleFormat::F32 => self.start_as::<f32>(writer),
            SampleFormat::F64 => self.start_as::<f64>(writer),
            SampleFormat::I8 => self.start_as::<i8>(writer),
            SampleFormat::I16 => self.start_as::<i16>(writer),
            SampleFormat::I24 => self.start_as::<cpal::I24>(writer),
            SampleFormat::I32 => self.start_as::<i32>(writer),
            SampleFormat::I64 => self.start_as::<i64>(writer),
            SampleFormat::U8 => self.start_as::<u8>(writer),
            SampleFormat::U16 => self.start_as::<u16>(writer),
            SampleFormat::U24 => self.start_as::<cpal::U24>(writer),
            SampleFormat::U32 => self.start_as::<u32>(writer),
            SampleFormat::U64 => self.start_as::<u64>(writer),
            format => Err(format!("unsupported input format {format}").into()),
        }
    }

    fn start_as<T>(self, mut writer: InputWriter) -> Result<InputStream, DeviceError>
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        let rate = self.sample_rate();
        let mut first = true;
        let lost = Arc::new(AtomicBool::new(false));
        let stream = self.device.build_input_stream::<T, _, _>(
            self.config,
            move |data: &[T], info| {
                if first {
                    first = false;
                    // Best effort: needs rtkit or an rtprio limit.
                    let _ = rt::set_realtime_priority(70);
                }
                let time = info.timestamp();
                let latency = time.callback.saturating_duration_since(time.capture);
                writer.set_latency_frames(duration_frames(latency, rate));
                writer.push(data.iter().map(|s| s.to_sample::<f32>()));
            },
            on_stream_error(&lost),
            None,
        )?;
        stream.play()?;
        Ok(InputStream {
            _stream: stream,
            lost,
        })
    }
}

/// A capturing input stream. Dropping it stops the stream.
pub struct InputStream {
    _stream: cpal::Stream,
    lost: Arc<AtomicBool>,
}

impl InputStream {
    /// Whether the stream stopped for good (e.g. the device was unplugged):
    /// open a device again to keep capturing.
    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }
}

/// The configuration to capture at `rate` with: the device's usual channels
/// and format if it can, as hosts may list channel counts that only make
/// sense when mixed (a sound server offers them all). The channels matter
/// most, as they decide what tracks can record from.
fn pick_input_config(
    supported: &[SupportedStreamConfigRange],
    channels: cpal::ChannelCount,
    format: SampleFormat,
    rate: u32,
) -> Option<&SupportedStreamConfigRange> {
    let closeness =
        |c: &SupportedStreamConfigRange| (c.channels() == channels, c.sample_format() == format);
    supported
        .iter()
        .filter(|c| can_convert(c.sample_format()))
        .filter(|c| (c.min_sample_rate()..=c.max_sample_rate()).contains(&rate))
        .max_by(|a, b| {
            closeness(a)
                .cmp(&closeness(b))
                .then_with(|| a.cmp_default_heuristics(b))
        })
}

/// Whether [`InputDevice::start`] can turn this format into `f32`.
fn can_convert(format: SampleFormat) -> bool {
    matches!(
        format,
        SampleFormat::F32
            | SampleFormat::F64
            | SampleFormat::I8
            | SampleFormat::I16
            | SampleFormat::I24
            | SampleFormat::I32
            | SampleFormat::I64
            | SampleFormat::U8
            | SampleFormat::U16
            | SampleFormat::U24
            | SampleFormat::U32
            | SampleFormat::U64
    )
}

fn buffer_range(size: &cpal::SupportedBufferSize) -> Option<(u32, u32)> {
    match *size {
        cpal::SupportedBufferSize::Range { min, max } => Some((min, max)),
        cpal::SupportedBufferSize::Unknown => None,
    }
}

fn duration_frames(duration: std::time::Duration, rate: f64) -> usize {
    (duration.as_secs_f64() * rate).round() as usize
}

/// Error callback of a stream, setting `lost` when it stopped for good.
fn on_stream_error(lost: &Arc<AtomicBool>) -> impl FnMut(cpal::Error) + Send + 'static {
    let lost = lost.clone();
    move |err| {
        if matches!(
            err.kind(),
            ErrorKind::DeviceNotAvailable
                | ErrorKind::HostUnavailable
                | ErrorKind::StreamInvalidated
        ) {
            lost.store(true, Ordering::Relaxed);
        }
        eprintln!("audio stream error: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(
        channels: u16,
        format: SampleFormat,
        min: u32,
        max: u32,
    ) -> SupportedStreamConfigRange {
        SupportedStreamConfigRange::new(
            channels,
            min,
            max,
            cpal::SupportedBufferSize::Unknown,
            format,
        )
    }

    fn pick(supported: &[SupportedStreamConfigRange], rate: u32) -> Option<(u16, SampleFormat)> {
        pick_input_config(supported, 8, SampleFormat::I32, rate)
            .map(|c| (c.channels(), c.sample_format()))
    }

    #[test]
    fn keeps_the_device_channels_and_format_when_the_rate_allows() {
        let supported = [
            range(2, SampleFormat::F32, 8000, 192000),
            range(8, SampleFormat::F32, 8000, 192000),
            range(8, SampleFormat::I32, 8000, 192000),
            range(32, SampleFormat::F32, 8000, 192000),
        ];
        assert_eq!(pick(&supported, 48000), Some((8, SampleFormat::I32)));
    }

    #[test]
    fn prefers_keeping_the_channels_over_the_format() {
        let supported = [
            range(2, SampleFormat::I32, 8000, 192000),
            range(8, SampleFormat::I16, 44100, 48000),
        ];
        assert_eq!(pick(&supported, 48000), Some((8, SampleFormat::I16)));
    }

    #[test]
    fn falls_back_to_what_runs_at_the_rate() {
        let supported = [
            range(8, SampleFormat::I32, 44100, 44100),
            range(2, SampleFormat::I16, 48000, 48000),
        ];
        assert_eq!(pick(&supported, 48000), Some((2, SampleFormat::I16)));
    }

    #[test]
    fn nothing_at_the_rate_or_in_a_usable_format() {
        let supported = [
            range(8, SampleFormat::I32, 44100, 44100),
            range(2, SampleFormat::DsdU8, 48000, 48000),
        ];
        assert_eq!(pick(&supported, 48000), None);
    }
}
