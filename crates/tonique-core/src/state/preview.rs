//! The browser's preview: a file played on its own, outside the
//! arrangement, while the transport is stopped.

use std::path::{Path, PathBuf};

use tonique_engine::edit::EditError;

use super::{PlaybackState, ProjectState, report};
use crate::audio::preview::FilePreview;

pub(super) struct Preview {
    pub(super) playback: PlaybackState,
    /// Where it plays, in frames of the file.
    pub(super) position: usize,
    /// The file last previewed.
    pub(super) path: Option<PathBuf>,
    /// Linear.
    pub(super) gain: f32,
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
    fn playing(&self) -> Option<&Path> {
        self.path
            .as_deref()
            .filter(|_| self.playback == PlaybackState::Playing)
    }

    /// Follows the engine: where it plays, and whether it still does.
    pub(super) fn follow(&mut self, position: Option<usize>, previewing: bool) {
        if let Some(position) = position {
            self.position = position;
        }
        if !previewing {
            // Played to the end.
            self.playback = PlaybackState::Paused;
        }
    }
}

impl ProjectState {
    pub fn pause_preview(&mut self) {
        self.preview.playback = PlaybackState::Paused;
        let result = self.session.engine_mut().preview_stop();
        report(result.map_err(EditError::from));
    }

    /// Play a file from the start. Ignored while the transport plays.
    pub fn play_preview(&mut self, path: PathBuf) {
        if self.transport.is_playing() {
            return;
        }
        let Some(source) = FilePreview::open(path.clone()) else {
            return;
        };
        self.preview.position = 0;
        self.preview.path = Some(path);
        self.preview.playback = PlaybackState::Playing;
        let engine = self.session.engine_mut();
        engine.preview_set_gain(self.preview.gain);
        let result = engine.preview_play(Box::new(source), 0);
        report(result.map_err(EditError::from));
    }

    /// How loud the preview plays (linear), now and next time.
    pub fn set_preview_gain(&mut self, gain: f32) {
        self.preview.gain = gain;
        self.session.engine_mut().preview_set_gain(gain);
    }

    /// Continue the last previewed file from `position` (in frames of the
    /// file).
    pub fn seek_preview(&mut self, position: usize) {
        self.preview.position = position;
        self.preview.playback = PlaybackState::Playing;
        let result = self.session.engine_mut().preview_seek(position);
        report(result.map_err(EditError::from));
    }

    pub fn preview_playback_state(&self) -> PlaybackState {
        self.preview.playback
    }

    pub fn preview_position(&self) -> usize {
        self.preview.position
    }

    /// The file the preview is playing, if it is.
    pub fn previewing(&self) -> Option<&Path> {
        self.preview.playing()
    }
}
