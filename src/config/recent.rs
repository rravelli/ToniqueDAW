//! Recently opened or saved projects, newest first, saved as `recent.json`
//! next to `settings.json`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::config::config_dir;

/// How many projects are remembered.
const MAX_RECENT: usize = 10;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecentProjects {
    paths: Vec<PathBuf>,
}

impl RecentProjects {
    pub fn load() -> Self {
        recent_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = recent_path() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, json);
        }
    }

    /// Newest first.
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Put `path` first, once, forgetting the oldest beyond the limit.
    pub fn add(&mut self, path: &Path) {
        self.remove(path);
        self.paths.insert(0, path.to_path_buf());
        self.paths.truncate(MAX_RECENT);
    }

    pub fn remove(&mut self, path: &Path) {
        self.paths.retain(|p| p != path);
    }

    pub fn clear(&mut self) {
        self.paths.clear();
    }
}

fn recent_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("recent.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_without_duplicates_up_to_the_limit() {
        let mut recent = RecentProjects::default();
        recent.add(Path::new("/a.tonique"));
        recent.add(Path::new("/b.tonique"));
        recent.add(Path::new("/a.tonique"));
        assert_eq!(
            recent.paths(),
            [Path::new("/a.tonique"), Path::new("/b.tonique")]
        );

        for i in 0..20 {
            recent.add(Path::new(&format!("/{i}.tonique")));
        }
        assert_eq!(recent.paths().len(), MAX_RECENT);
        assert_eq!(recent.paths()[0], Path::new("/19.tonique"));

        recent.remove(Path::new("/19.tonique"));
        assert_eq!(recent.paths()[0], Path::new("/18.tonique"));
        recent.clear();
        assert!(recent.paths().is_empty());
    }

    #[test]
    fn a_missing_or_broken_file_means_an_empty_list() {
        let recent: RecentProjects = serde_json::from_str("{}").unwrap();
        assert_eq!(recent, RecentProjects::default());
        assert!(serde_json::from_str::<RecentProjects>("not json").is_err());
    }
}
