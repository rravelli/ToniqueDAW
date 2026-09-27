use crate::config::{config_dir, keymap::Keymap};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tonique_engine::{device::DeviceOptions, engine::EngineConfig};

/// User preferences, saved as `settings.json` next to `config.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Interface zoom (1 = 100%).
    pub ui_scale: f32,
    /// Output device ID; `None` for the system default.
    pub device_id: Option<String>,
    /// `None` for the device's preferred rate.
    pub sample_rate: Option<u32>,
    /// Frames per device callback; `None` lets the device decide.
    pub buffer_frames: Option<u32>,
    /// Largest block the engine processes at once.
    pub block_size: usize,
    /// Helper threads for processing the graph in parallel (0 = off).
    pub worker_threads: usize,
    /// Graphs smaller than this run on the audio thread alone.
    pub parallel_threshold: usize,
    pub metronome_level: f32,
    /// Keyboard shortcuts that differ from the defaults.
    pub keymap: Keymap,
    /// Built-in theme name, or file name (without `.toml`) of a user theme.
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_scale: 1.,
            device_id: None,
            sample_rate: None,
            buffer_frames: None,
            block_size: 256,
            worker_threads: 2,
            parallel_threshold: EngineConfig::default().parallel_threshold,
            metronome_level: 0.4,
            keymap: Keymap::default(),
            theme: "dark".into(),
        }
    }
}

pub const UI_SCALE_RANGE: std::ops::RangeInclusive<f32> = 0.5..=2.5;

impl Settings {
    pub fn load() -> Self {
        settings_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = settings_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, json);
        }
    }

    pub fn device_options(&self) -> DeviceOptions {
        DeviceOptions {
            device_id: self.device_id.clone(),
            sample_rate: self.sample_rate,
            buffer_frames: self.buffer_frames,
        }
    }

    pub fn engine_config(&self, sample_rate: f64) -> EngineConfig {
        EngineConfig {
            sample_rate,
            max_block: self.block_size,
            worker_threads: self.worker_threads,
            parallel_threshold: self.parallel_threshold,
            ..Default::default()
        }
    }

    /// Copy the fields that need the audio restarted from `other`.
    pub fn set_audio(&mut self, other: &Settings) {
        self.device_id = other.device_id.clone();
        self.sample_rate = other.sample_rate;
        self.buffer_frames = other.buffer_frames;
        self.block_size = other.block_size;
        self.worker_threads = other.worker_threads;
        self.parallel_threshold = other.parallel_threshold;
    }

    /// Whether switching from `self` to `other` needs the audio restarted.
    pub fn audio_differs(&self, other: &Settings) -> bool {
        self.device_options() != other.device_options()
            || self.block_size != other.block_size
            || self.worker_threads != other.worker_threads
            || self.parallel_threshold != other.parallel_threshold
    }
}

fn settings_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_defaults() {
        let s: Settings = serde_json::from_str(r#"{ "ui_scale": 1.5 }"#).unwrap();
        assert_eq!(s.ui_scale, 1.5);
        assert_eq!(s.block_size, Settings::default().block_size);
        let round_trip: Settings =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(round_trip, s);
    }

    #[test]
    fn only_audio_fields_need_a_restart() {
        let a = Settings::default();
        let b = Settings {
            ui_scale: 2.,
            metronome_level: 0.1,
            ..a.clone()
        };
        assert!(!a.audio_differs(&b));
        let c = Settings {
            block_size: 512,
            ..a.clone()
        };
        assert!(a.audio_differs(&c));
    }

    #[test]
    fn set_audio_copies_exactly_the_audio_fields() {
        let other = Settings {
            ui_scale: 2.,
            metronome_level: 0.1,
            device_id: Some("dev".into()),
            block_size: 512,
            worker_threads: 0,
            ..Settings::default()
        };
        let mut s = Settings::default();
        s.set_audio(&other);
        assert!(!s.audio_differs(&other));
        assert_eq!(s.ui_scale, Settings::default().ui_scale);
        assert_eq!(s.metronome_level, Settings::default().metronome_level);
    }
}
