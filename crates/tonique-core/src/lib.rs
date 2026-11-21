//! Tonique without its UI: the project state, its commands and undo, the
//! grid, clips, audio sources and their analysis, the audio host, the
//! configuration and the browser's file tree. No UI toolkit: the app draws
//! it with egui, `tonique-gpui` with gpui.

pub mod analysis;
pub mod audio;
pub mod bench;
pub mod cache;
pub mod config;
pub mod core;
pub mod file_match;
pub mod file_tree;
pub mod utils;
pub mod waveform;
