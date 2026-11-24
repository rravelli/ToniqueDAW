//! The audio output, and reopening it after it's lost: a probe off the UI
//! thread every few seconds, then the project restarts the engine.

use std::{
    sync::mpsc::{Receiver, TryRecvError},
    time::{Duration, Instant},
};

use super::host::{AudioHost, probe_output};
use crate::config::settings::Settings;

/// How often to try opening an output again while there's none.
const AUDIO_RETRY: Duration = Duration::from_secs(2);

/// The audio device, and reopening it after it's lost: a probe off the UI
/// thread every few seconds, then [`ProjectState`] restarts the engine.
#[derive(Default)]
pub(crate) struct AudioOutput {
    /// `None` without an audio device (e.g. in tests).
    pub(crate) host: Option<AudioHost>,
    /// Why the last audio restart failed, if it did.
    pub(crate) error: Option<String>,
    /// When to try opening the output again, while there's none.
    pub(crate) reconnect_at: Option<Instant>,
    /// Whether an output can be opened, checked off the UI thread before
    /// reconnecting.
    pub(crate) probe: Option<Receiver<bool>>,
}

impl AudioOutput {
    /// Whether there's no output: it was lost and is being reopened.
    pub(crate) fn lost(&self) -> bool {
        self.reconnect_at.is_some()
    }

    /// Whether the device went away (e.g. unplugged) since it was opened.
    pub(crate) fn disconnected(&self) -> bool {
        self.host.as_ref().is_some_and(AudioHost::is_lost)
    }

    /// Without an output, for `error`: tried again in a moment.
    pub(crate) fn lose(&mut self, error: String) {
        self.host = None;
        self.error = Some(error);
        self.retry_later();
    }

    pub(crate) fn retry_later(&mut self) {
        self.reconnect_at = Some(Instant::now() + AUDIO_RETRY);
    }

    pub(crate) fn reconnected(&mut self) {
        self.reconnect_at = None;
        self.error = None;
    }

    /// Probes for an output once it's time to try again; whether one can
    /// be opened, once the probe answers.
    pub(crate) fn poll_probe(&mut self, settings: &Settings) -> Option<bool> {
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
