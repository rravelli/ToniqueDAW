use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Serialize, Deserialize)]
struct Config {
    last_dir: String,
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
