//! How durations, levels and sizes read.

use std::path::Path;

use tonique_engine::sample::WavFormat;

use crate::core::{project::project_name, state::ProjectState};

/// `48 kHz  ·  stereo`: what the engine renders.
pub(super) fn audio_format(state: &ProjectState) -> String {
    let config = state.engine_config();
    let channels = match config.output_channels {
        1 => "mono".to_string(),
        2 => "stereo".to_string(),
        n => format!("{n} channels"),
    };
    format!("{} kHz  ·  {channels}", trimmed(config.sample_rate / 1000.))
}

pub(super) fn peak(level: f32) -> String {
    match level {
        0. => "silent".to_string(),
        level => format!("peak {}", decibels(level)),
    }
}

pub(super) fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || project_name(path),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// `m:ss`, or `m:ss.s` under ten seconds.
pub(super) fn duration(seconds: f64) -> String {
    let minutes = (seconds / 60.).floor();
    let rest = seconds - minutes * 60.;
    if seconds < 10. {
        format!("{minutes:.0}:{rest:04.1}")
    } else {
        format!("{minutes:.0}:{:02.0}", rest.floor())
    }
}

/// A level in dB relative to full scale.
pub(super) fn decibels(level: f32) -> String {
    let db = 20. * level.log10();
    if db > 0. {
        format!("+{db:.1} dBFS")
    } else {
        format!("{db:.1} dBFS").replace('-', "−")
    }
}

/// `48`, `44.1`: without a trailing `.0`.
pub(super) fn trimmed(value: f64) -> String {
    let text = format!("{value:.1}");
    text.strip_suffix(".0").map_or(text.clone(), str::to_string)
}

pub(super) fn sample_bytes(format: WavFormat) -> usize {
    match format {
        WavFormat::Int16 => 2,
        WavFormat::Int24 => 3,
        WavFormat::Float32 => 4,
    }
}

pub(super) fn file_size(bytes: f64) -> String {
    match bytes {
        b if b >= 1e9 => format!("{:.1} GB", b / 1e9),
        b if b >= 1e6 => format!("{:.1} MB", b / 1e6),
        b => format!("{:.0} kB", (b / 1e3).max(1.)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_as_minutes_and_seconds() {
        assert_eq!(duration(2.5), "0:02.5");
        assert_eq!(duration(75.9), "1:15");
        assert_eq!(duration(600.), "10:00");
    }

    #[test]
    fn levels_sizes_and_rates_read_naturally() {
        assert_eq!(decibels(1.), "0.0 dBFS");
        assert_eq!(decibels(0.5), "−6.0 dBFS");
        assert_eq!(decibels(2.), "+6.0 dBFS");
        assert_eq!(file_size(12_345_678.), "12.3 MB");
        assert_eq!(file_size(10.), "1 kB");
        assert_eq!((trimmed(48.), trimmed(44.1)), ("48".into(), "44.1".into()));
    }
}
