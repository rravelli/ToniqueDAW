use std::path::PathBuf;

use crate::core::{
    message::GuiToPlayerMsg,
    state::{PlaybackState, ToniqueProjectState},
};

impl ToniqueProjectState {
    /// Set BPM value
    pub fn set_bpm(&mut self, value: f32) {
        self.bpm = value.clamp(10., 999.9);
        let _ = self.tx.push(GuiToPlayerMsg::UpdateBPM(value));
    }
    /// Get BPM value
    pub fn bpm(&self) -> f32 {
        self.bpm
    }
    /// Set playback position in beats.
    pub fn set_playback_position(&mut self, value: f32) {
        self.playback_position = value.max(0.);
        let _ = self.tx.push(GuiToPlayerMsg::SeekTo(value));
    }
    /// Get playback position in beats.
    pub fn playback_position(&self) -> f32 {
        self.playback_position
    }
    /// Pause playback
    pub fn pause(&mut self) {
        self.playback_state = PlaybackState::Paused;
        let _ = self.tx.push(GuiToPlayerMsg::Pause);
    }
    /// Start playback
    pub fn play(&mut self) {
        self.playback_state = PlaybackState::Playing;
        self.preview_playback_state = PlaybackState::Paused;
        let _ = self.tx.push(GuiToPlayerMsg::Play);
    }
    /// Pause preview playback
    pub fn pause_preview(&mut self) {
        self.preview_playback_state = PlaybackState::Paused;
        let _ = self.tx.push(GuiToPlayerMsg::PausePreview());
    }
    /// Start preview playback
    pub fn play_preview(&mut self, path: PathBuf) {
        self.preview_playback_state = PlaybackState::Playing;
        let _ = self.tx.push(GuiToPlayerMsg::PlayPreview(path));
    }
    /// Seek preview playback to specified position
    pub fn seek_preview(&mut self, pos: usize) {
        self.preview_position = pos;
        self.preview_playback_state = PlaybackState::Playing;
        let _ = self.tx.push(GuiToPlayerMsg::SeekPreview(pos));
    }
    /// Toggle metronome state
    pub fn toggle_metronome(&mut self) {
        self.metronome = !self.metronome;
        let _ = self
            .tx
            .push(GuiToPlayerMsg::ToggleMetronome(self.metronome));
    }
    /// Get metronome state
    pub fn metronome(&self) -> bool {
        self.metronome
    }
    /// Get playback state
    pub fn playback_state(&self) -> PlaybackState {
        self.playback_state
    }
    /// Get preview playback state
    pub fn preview_playback_state(&self) -> PlaybackState {
        self.preview_playback_state
    }
    /// Get preview playback position in samples
    pub fn preview_position(&self) -> usize {
        self.preview_position
    }
}
