//! Tonique without its UI: the project state, its commands and undo, the
//! arrangement and its grid, effects, project files, export, audio files
//! and their analysis, the audio output, the configuration and the
//! browser's model. No UI toolkit: the app draws it with egui,
//! `tonique-gpui` with gpui.

pub mod arrangement;
pub mod audio;
pub mod audio_files;
pub mod bench;
pub mod browser;
pub mod config;
pub mod diagnostics;
pub mod effect;
pub mod export;
pub mod project;
pub mod state;
pub mod utils;
