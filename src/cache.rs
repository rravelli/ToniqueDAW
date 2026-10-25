use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::sync::Arc;

use crate::analysis::{AudioInfo, read_audio_info};

// Inner struct to keep cache logic encapsulated
pub struct AudioAnalysisCache {
    inner: DashMap<PathBuf, AudioInfo>,
}

impl AudioAnalysisCache {
    pub fn get_or_analyze(&self, path: PathBuf) -> Option<AudioInfo> {
        if let Some(result) = self.inner.get(&path) {
            return Some(result.clone());
        }

        // Forget files nothing uses anymore (e.g. browsed past) so their
        // data can be freed; they're cheap to analyze again.
        self.inner
            .retain(|_, info| Arc::strong_count(&info.data) > 1);
        let result = read_audio_info(path.clone());

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
