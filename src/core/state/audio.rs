//! The audio output and the settings it's opened with.

use crate::{
    audio::host::{AudioHost, probe_output, start_audio},
    config::settings::Settings,
};
use std::{sync::mpsc::TryRecvError, time::Instant};

use super::{AUDIO_RETRY, PlaybackState, ProjectState};

impl ProjectState {
    /// Take over the running audio output and the settings it was opened
    /// with. Without one (no device worked), keep trying to open it.
    pub fn attach_audio(&mut self, audio: Result<AudioHost, String>, settings: Settings) {
        match audio {
            Ok(audio) => self.audio = Some(audio),
            Err(e) => self.lose_audio(e),
        }
        self.settings = settings;
        self.apply_metronome_level();
    }
    /// Whether there's no audio output: it was lost and is being reopened.
    pub fn audio_lost(&self) -> bool {
        self.reconnect_at.is_some()
    }
    /// Stop retrying the output tests don't have, so the app shows no
    /// warning about it (for screenshots).
    #[cfg(test)]
    pub fn ignore_missing_audio(&mut self) {
        self.reconnect_at = None;
        self.probe = None;
        self.audio_error = None;
    }
    /// Notice a lost output (e.g. an unplugged device) and reopen one
    /// every few seconds, falling back to the default device.
    pub(super) fn watch_audio(&mut self) {
        if self.audio.as_ref().is_some_and(AudioHost::is_lost) {
            self.lose_audio("the audio device was disconnected".into());
        }
        if self.probe.is_none() && self.reconnect_at.is_some_and(|at| Instant::now() >= at) {
            self.probe = Some(probe_output(&self.settings));
        }
        let Some(probe) = &self.probe else {
            return;
        };
        let available = match probe.try_recv() {
            Ok(available) => available,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => false,
        };
        self.probe = None;
        if !self.audio_lost() {
            // Reconnected meanwhile (e.g. from the settings).
            return;
        }
        if !available {
            self.reconnect_at = Some(Instant::now() + AUDIO_RETRY);
            return;
        }
        match self.restart_audio() {
            Ok(()) => {
                self.reconnect_at = None;
                self.audio_error = None;
            }
            Err(e) => self.lose_audio(e),
        }
    }
    fn lose_audio(&mut self, error: String) {
        if self.playback_state == PlaybackState::Playing {
            self.stop();
        }
        self.audio = None;
        self.audio_error = Some(error);
        self.reconnect_at = Some(Instant::now() + AUDIO_RETRY);
    }
    pub fn audio(&self) -> Option<&AudioHost> {
        self.audio.as_ref()
    }
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    /// The engine running now (its settings may differ from the saved ones
    /// until an audio restart).
    pub fn engine_config(&self) -> tonique_engine::engine::EngineConfig {
        *self.session.engine().config()
    }
    pub fn cpu_load(&self) -> f32 {
        self.session.engine().cpu_load()
    }
    /// Save and apply `settings`, restarting audio if device or engine
    /// settings changed. If the new audio settings don't work, the previous
    /// ones are restored and `audio_error` says why.
    pub fn apply_settings(&mut self, settings: Settings) {
        let previous = std::mem::replace(&mut self.settings, settings);
        self.apply_metronome_level();
        let has_output = self.audio.is_some() || self.audio_lost();
        if has_output && previous.audio_differs(&self.settings) {
            self.audio_error = None;
            if let Err(e) = self.restart_audio() {
                self.audio_error = Some(e);
                let attempted = std::mem::replace(&mut self.settings, previous);
                // Keep the non-audio changes.
                self.settings.ui_scale = attempted.ui_scale;
                self.settings.metronome_level = attempted.metronome_level;
                if let Err(e) = self.restart_audio() {
                    let error = format!(
                        "{}; restoring the previous device failed too: {e}",
                        self.audio_error.take().unwrap_or_default()
                    );
                    self.lose_audio(error);
                }
            }
            if self.audio.is_some() {
                self.reconnect_at = None;
            }
        }
        self.settings.save();
    }

    /// Reopen the output device and move the project to a fresh engine with
    /// the current settings. Keeps the undo history and the playhead; keeps
    /// playing unless the sample rate changed (files are converted again).
    fn restart_audio(&mut self) -> Result<(), String> {
        let position = self.playhead;
        let playing = self.playback_state == PlaybackState::Playing;
        self.pause_preview();
        // Release the device first: some drivers only allow one stream.
        self.audio = None;
        let (audio, engine) = start_audio(&self.settings).map_err(|e| e.to_string())?;
        let rate = engine.config().sample_rate;
        let rate_changed = rate != self.session.engine().config().sample_rate;
        let old_engine = self
            .session
            .replace_engine(engine)
            .map_err(|e| e.to_string())?;
        drop(old_engine);
        self.audio = Some(audio);
        self.sources.set_engine_rate(rate);
        self.graph.reset();
        self.seek(position);
        if playing && !rate_changed {
            self.play();
        } else {
            self.playback_state = PlaybackState::Paused;
        }
        Ok(())
    }
}
