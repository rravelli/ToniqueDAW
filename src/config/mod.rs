use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Default, Debug)]
pub struct Config {
    directories: Vec<PathBuf>,
}

// Helper functions for serde to convert PathBuf <-> String
mod serde_pathbuf_vec {
    use serde::{self, Deserialize, Deserializer, Serialize, Serializer};
    use std::path::PathBuf;

    pub fn serialize<S>(dirs: &Vec<PathBuf>, serializer: S) -> Result<S::Ok, S::Error>
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
}

impl Config {
    /// Load config from disk
    pub fn load() -> Self {
        if let Some(path) = get_config_path() {
            if let Ok(data) = fs::read_to_string(&path) {
                if let Ok(helper) = serde_json::from_str::<ConfigSerdeHelper>(&data) {
                    return Config {
                        directories: helper.directories,
                    };
                }
            }
        }
        Config::default()
    }

    /// Save config to disk
    pub fn save(&self) {
        if let Some(path) = get_config_path() {
            println!("{:?}", path);
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }

            let helper = ConfigSerdeHelper {
                directories: self.directories.clone(),
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
        self.save();
    }

    /// Return a clone of all directories
    pub fn list_dirs(&self) -> Vec<PathBuf> {
        self.directories.clone()
    }
}

/// Returns the configuration file path.
fn get_config_path() -> Option<PathBuf> {
    ProjectDirs::from("com", "Bytenosis", "Tonique")
        .map(|proj_dirs| proj_dirs.config_dir().join("config.json"))
}
