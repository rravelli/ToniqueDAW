//! The browser's side: audio files as assets. The folders as a tree,
//! finding missing files among them, decoding files and their peaks,
//! their rhythm and tempo (cached across projects), streaming one for the
//! preview, and the files the project's clips use, loaded for the engine.

pub mod analysis;
pub mod cache;
pub mod file_match;
pub mod file_tempo;
pub mod file_tree;
pub mod preview;
pub mod sources;
pub mod waveform;
