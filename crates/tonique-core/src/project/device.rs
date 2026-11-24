//! The project's side of the audio output (`crate::device`): attaching
//! it, restarting it with new settings, reopening it once lost.

use crate::{
    config::settings::Settings,
    device::host::{AudioHost, Startup, start_audio},
};

use super::{PlaybackState, ProjectState};

impl ProjectState {
    /// A new project on the audio the app started with.
    pub fn started(startup: Startup) -> Self {
        let mut state = Self::new(startup.engine);
        state.attach_audio(startup.audio, startup.settings);
        state
    }

    /// Take over the running audio output and the settings it was opened
    /// with. Without one (no device worked), keep trying to open it.
    pub fn attach_audio(&mut self, audio: Result<AudioHost, String>, settings: Settings) {
        match audio {
            Ok(audio) => self.output.host = Some(audio),
            Err(e) => self.lose_audio(e),
        }
        self.settings = settings;
        self.apply_metronome_level();
    }
    /// Whether there's no audio output: it was lost and is being reopened.
    pub fn audio_lost(&self) -> bool {
        self.output.lost()
    }
    /// Stop retrying the output tests don't have, so the app shows no
    /// warning about it (for screenshots).
    #[cfg(any(test, feature = "test-support"))]
    pub fn ignore_missing_audio(&mut self) {
        self.output.reconnect_at = None;
        self.output.probe = None;
        self.output.error = None;
    }
    /// Notice a lost output (e.g. an unplugged device) and reopen one
    /// every few seconds, falling back to the default device.
    pub(super) fn watch_audio(&mut self) {
        if self.output.disconnected() {
            self.lose_audio("the audio device was disconnected".into());
        }
        let Some(available) = self.output.poll_probe(&self.settings) else {
            return;
        };
        if !self.output.lost() {
            // Reconnected meanwhile (e.g. from the settings).
            return;
        }
        if !available {
            self.output.retry_later();
            return;
        }
        match self.restart_audio() {
            Ok(()) => self.output.reconnected(),
            Err(e) => self.lose_audio(e),
        }
    }
    fn lose_audio(&mut self, error: String) {
        if self.transport.is_playing() {
            self.stop();
        }
        self.output.lose(error);
    }
    /// Why the last audio restart failed, if it did.
    pub fn audio_error(&self) -> Option<&str> {
        self.output.error.as_deref()
    }
    pub fn audio(&self) -> Option<&AudioHost> {
        self.output.host.as_ref()
    }
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    /// The engine running now (its settings may differ from the saved ones
    /// until an audio restart).
    pub fn engine_config(&self) -> tonique_engine::engine::EngineConfig {
        *self.edit_session.engine().config()
    }
    pub fn cpu_load(&self) -> f32 {
        self.edit_session.engine().cpu_load()
    }
    /// Save and apply `settings`, restarting audio if device or engine
    /// settings changed. If the new audio settings don't work, the previous
    /// ones are restored and `audio_error` says why.
    pub fn apply_settings(&mut self, settings: Settings) {
        let previous = std::mem::replace(&mut self.settings, settings);
        self.apply_metronome_level();
        let has_output = self.output.host.is_some() || self.audio_lost();
        if has_output && previous.audio_differs(&self.settings) {
            self.output.error = None;
            if let Err(e) = self.restart_audio() {
                self.output.error = Some(e);
                let attempted = std::mem::replace(&mut self.settings, previous);
                // Keep the non-audio changes.
                self.settings.ui_scale = attempted.ui_scale;
                self.settings.metronome_level = attempted.metronome_level;
                if let Err(e) = self.restart_audio() {
                    let error = format!(
                        "{}; restoring the previous device failed too: {e}",
                        self.output.error.take().unwrap_or_default()
                    );
                    self.lose_audio(error);
                }
            }
            if self.output.host.is_some() {
                self.output.reconnect_at = None;
            }
        }
        self.settings.save();
    }

    /// Reopen the output device and move the project to a fresh engine with
    /// the current settings. Keeps the undo history and the playhead; keeps
    /// playing unless the sample rate changed (files are converted again).
    fn restart_audio(&mut self) -> Result<(), String> {
        let position = self.transport.playhead;
        let playing = self.transport.playback == PlaybackState::Playing;
        self.pause_preview();
        // Release the device first: some drivers only allow one stream.
        self.output.host = None;
        let (audio, engine) = start_audio(&self.settings).map_err(|e| e.to_string())?;
        let rate = engine.config().sample_rate;
        let rate_changed = rate != self.edit_session.engine().config().sample_rate;
        let old_engine = self
            .edit_session
            .replace_engine(engine)
            .map_err(|e| e.to_string())?;
        drop(old_engine);
        self.output.host = Some(audio);
        self.sources.set_engine_rate(rate);
        self.monitor.graph.reset();
        self.seek(position);
        if playing && !rate_changed {
            self.play();
        } else {
            self.transport.playback = PlaybackState::Paused;
        }
        Ok(())
    }
}
