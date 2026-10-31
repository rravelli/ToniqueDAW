pub mod keymap;
pub mod labels;
pub mod recent;
pub mod settings;

use labels::Labels;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct Config {
    /// The browser's folders, in its order.
    directories: Vec<PathBuf>,
    /// Those of them shown collapsed.
    collapsed: Vec<PathBuf>,
    /// Selecting an audio file in the browser plays it.
    auto_preview: bool,
    preview_volume: f32,
    labels: Labels,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            directories: Vec::new(),
            collapsed: Vec::new(),
            auto_preview: true,
            preview_volume: 1.,
            labels: Labels::default(),
        }
    }
}

// Helper functions for serde to convert PathBuf <-> String
mod serde_pathbuf_vec {
    use serde::{self, Deserialize, Deserializer, Serialize, Serializer};
    use std::path::PathBuf;

    pub fn serialize<S>(dirs: &[PathBuf], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let strings: Vec<String> = dirs
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        strings.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<PathBuf>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let strings = Vec::<String>::deserialize(deserializer)?;
        Ok(strings.into_iter().map(PathBuf::from).collect())
    }
}

// Helper struct for serialization only
#[derive(Serialize, Deserialize)]
struct ConfigSerdeHelper {
    #[serde(with = "serde_pathbuf_vec")]
    directories: Vec<PathBuf>,
    #[serde(default, with = "serde_pathbuf_vec")]
    collapsed: Vec<PathBuf>,
    #[serde(default = "yes")]
    auto_preview: bool,
    #[serde(default = "full")]
    preview_volume: f32,
    #[serde(default)]
    labels: Labels,
}

fn yes() -> bool {
    true
}

fn full() -> f32 {
    1.
}

impl Config {
    /// Load config from disk
    pub fn load() -> Self {
        if let Some(path) = config_path()
            && let Ok(data) = fs::read_to_string(&path)
            && let Ok(helper) = serde_json::from_str::<ConfigSerdeHelper>(&data)
        {
            return Config {
                directories: helper.directories,
                collapsed: helper.collapsed,
                auto_preview: helper.auto_preview,
                preview_volume: helper.preview_volume.clamp(0., 1.),
                labels: helper.labels,
            };
        }
        Config::default()
    }

    /// Save config to disk
    pub fn save(&self) {
        if let Some(path) = config_path() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }

            let helper = ConfigSerdeHelper {
                directories: self.directories.clone(),
                collapsed: self.collapsed.clone(),
                auto_preview: self.auto_preview,
                preview_volume: self.preview_volume,
                labels: self.labels.clone(),
            };

            if let Ok(json) = serde_json::to_string_pretty(&helper) {
                let _ = fs::write(path, json);
            }
        }
    }

    /// Add a directory if it doesn't exist
    pub fn add_dir(&mut self, dir: impl Into<PathBuf>) {
        let dir = dir.into();
        if !self.directories.contains(&dir) {
            self.directories.push(dir);
            self.save();
        }
    }

    /// Remove a directory
    pub fn remove_dir(&mut self, dir: &Path) {
        self.directories.retain(|d| d != dir);
        self.collapsed.retain(|d| d != dir);
        self.save();
    }

    /// Whether selecting an audio file in the browser plays it.
    pub fn auto_preview(&self) -> bool {
        self.auto_preview
    }

    pub fn set_auto_preview(&mut self, on: bool) {
        self.auto_preview = on;
        self.save();
    }

    /// Where the browser's preview volume slider is, from 0 to 1.
    pub fn preview_volume(&self) -> f32 {
        self.preview_volume
    }

    pub fn set_preview_volume(&mut self, volume: f32) {
        self.preview_volume = volume;
        self.save();
    }

    pub fn labels(&self) -> &Labels {
        &self.labels
    }

    /// Change the labels, and save them.
    pub fn edit_labels(&mut self, edit: impl FnOnce(&mut Labels)) {
        edit(&mut self.labels);
        self.save();
    }

    /// The browser's folders shown collapsed.
    pub fn collapsed_dirs(&self) -> &[PathBuf] {
        &self.collapsed
    }

    /// Remember which folders are collapsed, saving if that changed.
    pub fn set_collapsed_dirs(&mut self, collapsed: Vec<PathBuf>) {
        if collapsed != self.collapsed {
            self.collapsed = collapsed;
            self.save();
        }
    }

    /// Return a clone of all directories
    pub fn list_dirs(&self) -> Vec<PathBuf> {
        self.directories.clone()
    }
}

/// Returns the configuration file path.
fn config_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("config.json"))
}

/// Where Tonique keeps its configuration, settings and themes.
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(test)]
    return Some(test_config_dir());
    #[cfg(not(test))]
    directories::ProjectDirs::from("com", "Bytenosis", "Tonique")
        .map(|dirs| dirs.config_dir().to_path_buf())
}

/// Tests keep their configuration away from the user's: an empty directory
/// of their own for each thread (each test runs on one), made when first
/// asked for.
#[cfg(test)]
pub fn test_config_dir() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    thread_local! {
        static DIR: PathBuf = {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("tonique-tests-{}", std::process::id()))
                .join(n.to_string());
            let _ = std::fs::remove_dir_all(&dir);
            dir
        };
    }
    DIR.with(Clone::clone)
}
