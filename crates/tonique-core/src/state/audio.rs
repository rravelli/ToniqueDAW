//! The audio output and the settings it's opened with.

use crate::{
    audio::host::{AudioHost, Startup, probe_output, start_audio},
    config::settings::Settings,
};
use std::{
    sync::mpsc::{Receiver, TryRecvError},
    time::Instant,
};

use super::{AUDIO_RETRY, PlaybackState, ProjectState};

/// The audio device, and reopening it after it's lost: a probe off the UI
/// thread every few seconds, then [`ProjectState`] restarts the engine.
#[derive(Default)]
pub(super) struct AudioOutput {
    /// `None` without an audio device (e.g. in tests).
    pub(super) host: Option<AudioHost>,
    /// Why the last audio restart failed, if it did.
    pub(super) error: Option<String>,
    /// When to try opening the output again, while there's none.
    pub(super) reconnect_at: Option<Instant>,
    /// Whether an output can be opened, checked off the UI thread before
    /// reconnecting.
    pub(super) probe: Option<Receiver<bool>>,
}

impl AudioOutput {
    /// Whether there's no output: it was lost and is being reopened.
    pub(super) fn lost(&self) -> bool {
        self.reconnect_at.is_some()
    }

    /// Whether the device went away (e.g. unplugged) since it was opened.
    fn disconnected(&self) -> bool {
        self.host.as_ref().is_some_and(AudioHost::is_lost)
    }

    /// Without an output, for `error`: tried again in a moment.
    fn lose(&mut self, error: String) {
        self.host = None;
        self.error = Some(error);
        self.retry_later();
    }

    fn retry_later(&mut self) {
        self.reconnect_at = Some(Instant::now() + AUDIO_RETRY);
    }

    fn reconnected(&mut self) {
        self.reconnect_at = None;
        self.error = None;
    }

    /// Probes for an output once it's time to try again; whether one can
    /// be opened, once the probe answers.
    fn poll_probe(&mut self, settings: &Settings) -> Option<bool> {
        if self.probe.is_none() && self.reconnect_at.is_some_and(|at| Instant::now() >= at) {
            self.probe = Some(probe_output(settings));
        }
        let available = match self.probe.as_ref()?.try_recv() {
            Ok(available) => available,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => false,
        };
        self.probe = None;
        Some(available)
    }
}

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
        self.diagnostics.graph.reset();
        self.seek(position);
        if playing && !rate_changed {
            self.play();
        } else {
            self.transport.playback = PlaybackState::Paused;
        }
        Ok(())
    }
}
