use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::path::PathBuf;

use crate::analysis::{AudioInfo, get_audio_info};

// Inner struct to keep cache logic encapsulated
pub struct AudioAnalysisCache {
    inner: DashMap<PathBuf, AudioInfo>,
}

impl AudioAnalysisCache {
    pub fn get_or_analyze(&self, path: PathBuf) -> Option<AudioInfo> {
        if let Some(result) = self.inner.get(&path) {
            return Some(result.clone());
        }

        let result = get_audio_info(path.clone());

        match result {
            Ok(info) => {
                self.inner.insert(path.clone(), info.clone());
                Some(info)
            }
            Err(_) => None,
        }
    }
}

// Static global instance
pub static AUDIO_ANALYSIS_CACHE: Lazy<AudioAnalysisCache> = Lazy::new(|| AudioAnalysisCache {
    inner: DashMap::new(),
});
