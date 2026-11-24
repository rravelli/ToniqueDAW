//! The browser's preview: a file played on its own, outside the
//! arrangement, while the transport is stopped.

use std::path::{Path, PathBuf};

use super::PlaybackState;

pub(crate) struct Preview {
    pub(crate) playback: PlaybackState,
    /// Where it plays, in frames of the file.
    pub(crate) position: usize,
    /// The file last previewed.
    pub(crate) path: Option<PathBuf>,
    /// Linear.
    pub(crate) gain: f32,
}

impl Default for Preview {
    fn default() -> Self {
        Self {
            playback: PlaybackState::Paused,
            position: 0,
            path: None,
            gain: 1.,
        }
    }
}

impl Preview {
    /// The file playing, if one is.
    pub(crate) fn playing(&self) -> Option<&Path> {
        self.path
            .as_deref()
            .filter(|_| self.playback == PlaybackState::Playing)
    }

    /// Follows the engine: where it plays, and whether it still does.
    pub(crate) fn follow(&mut self, position: Option<usize>, previewing: bool) {
        if let Some(position) = position {
            self.position = position;
        }
        if !previewing {
            // Played to the end.
            self.playback = PlaybackState::Paused;
        }
    }
}
