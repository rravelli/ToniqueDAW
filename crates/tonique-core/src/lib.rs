//! Tonique without its UI. No UI toolkit: the app draws it with egui,
//! `tonique-gpui` with gpui. Laid out by lifecycle:
//!
//! - [`project`]: `ProjectState`, the facade the UIs drive, running what
//!   involves several of the parts below;
//! - [`document`]: views of the engine's edit and what's saved with it;
//! - [`files`]: new, open, save, recovery, and export;
//! - [`session`]: what the user works on, never saved (selection,
//!   clipboard, the grid's zoom and scroll, the rows' layout);
//! - [`transport`]: playhead, loop, metronome, preview;
//! - [`monitor`]: meters, load, the graph view's data;
//! - [`device`]: the audio output;
//! - [`library`]: audio files as assets, for the browser and the clips;
//! - [`config`]: settings, keymap, recent projects; [`dev`]: bench, memory.

pub mod config;
pub mod dev;
pub mod device;
pub mod document;
pub mod files;
pub mod library;
pub mod monitor;
pub mod project;
pub mod session;
pub mod transport;
pub mod utils;
