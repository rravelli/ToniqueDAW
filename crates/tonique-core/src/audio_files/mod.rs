//! Audio files: decoding them and their waveform peaks, analysing their
//! rhythm and tempo (cached across projects), and the memory they take.

pub mod analysis;
pub mod cache;
pub mod file_tempo;
pub mod memory;
pub mod waveform;
