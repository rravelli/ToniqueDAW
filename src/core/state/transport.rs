use std::path::PathBuf;

use crate::core::{
    message::GuiToPlayerMsg,
    state::{PlaybackState, ToniqueProjectState},
};

impl ToniqueProjectState {
    /// Set BPM value
    pub fn set_bpm(&mut self, value: f32) {
        self.transport.bpm = value.clamp(10., 999.9);
        let _ = self.tx.push(GuiToPlayerMsg::UpdateBPM(value));
    }
    /// Get BPM value
    pub fn bpm(&self) -> f32 {
        self.transport.bpm
    }
    /// Set playback position in beats.
    pub fn set_playback_position(&mut self, value: f32) {
        self.transport.playback_position = value.max(0.);
        let _ = self.tx.push(GuiToPlayerMsg::SeekTo(value));
    }
    /// Get playback position in beats.
    pub fn playback_position(&self) -> f32 {
        self.transport.playback_position
    }
    /// Pause playback
    pub fn pause(&mut self) {
        self.transport.playback_state = PlaybackState::Paused;
        let _ = self.tx.push(GuiToPlayerMsg::Pause);
    }
    /// Start playback
    pub fn play(&mut self) {
        self.transport.playback_state = PlaybackState::Playing;
        self.transport.preview_playback_state = PlaybackState::Paused;
        let _ = self.tx.push(GuiToPlayerMsg::Play);
    }
    /// Pause preview playback
    pub fn pause_preview(&mut self) {
        self.transport.preview_playback_state = PlaybackState::Paused;
        let _ = self.tx.push(GuiToPlayerMsg::PausePreview());
    }
    /// Start preview playback
    pub fn play_preview(&mut self, path: PathBuf) {
        self.transport.preview_playback_state = PlaybackState::Playing;
        let _ = self.tx.push(GuiToPlayerMsg::PlayPreview(path));
    }
    /// Seek preview playback to specified position
    pub fn seek_preview(&mut self, pos: usize) {
        self.transport.preview_position = pos;
        self.transport.preview_playback_state = PlaybackState::Playing;
        let _ = self.tx.push(GuiToPlayerMsg::SeekPreview(pos));
    }
    /// Toggle metronome state
    pub fn toggle_metronome(&mut self) {
        self.transport.metronome = !self.transport.metronome;
        let _ = self
            .tx
            .push(GuiToPlayerMsg::ToggleMetronome(self.transport.metronome));
    }
    /// Get metronome state
    pub fn metronome(&self) -> bool {
        self.transport.metronome
    }
    /// Get playback state
    pub fn playback_state(&self) -> PlaybackState {
        self.transport.playback_state
    }
    /// Get preview playback state
    pub fn preview_playback_state(&self) -> PlaybackState {
        self.transport.preview_playback_state
    }
    /// Get preview playback position in samples
    pub fn preview_position(&self) -> usize {
        self.transport.preview_position
    }
}

pub struct TransportState {
    pub playback_position: f32,
    pub playback_state: PlaybackState,
    pub preview_playback_state: PlaybackState,
    pub preview_position: usize,
    pub metronome: bool,
    pub bpm: f32,
}

impl TransportState {
    pub fn new() -> Self {
        Self {
            playback_position: 0.,
            playback_state: PlaybackState::Paused,
            preview_playback_state: PlaybackState::Paused,
            preview_position: 0,
            metronome: false,
            bpm: 120.,
        }
    }

    pub fn update(&mut self, delta: f32) {
        if self.playback_state == PlaybackState::Playing {
            self.playback_position += delta * self.bpm / 60.;
        }
    }
}
