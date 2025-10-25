use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

struct Config {
    last_dir: String,
}

impl Serialize for Config {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.last_dir)
    }
}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let last_dir = String::deserialize(deserializer)?;
        Ok(Config { last_dir })
    }
}

fn get_config_path() -> Option<PathBuf> {
    ProjectDirs::from("com", "Bytenosis", "Tonique")
        .map(|proj_dirs| proj_dirs.config_dir().join("config.json"))
}

pub fn save_work_dir(dir: &str) {
    if let Some(path) = get_config_path() {
        let config = Config {
            last_dir: dir.to_string(),
        };
        fs::create_dir_all(path.parent().unwrap()).ok();
        fs::write(path, serde_json::to_string(&config).unwrap()).ok();
    }
}

pub fn load_work_dir() -> Option<String> {
    get_config_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|json| serde_json::from_str::<Config>(&json).ok())
        .map(|cfg| cfg.last_dir)
}
