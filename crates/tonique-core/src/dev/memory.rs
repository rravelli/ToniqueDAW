//! How much memory audio files' data takes, for a debug readout.

use crate::library::analysis::AudioData;

/// Audio files' decoded samples, and their waveform peaks and onsets.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct AudioMemory {
    pub files: usize,
    /// Of the files fully decoded: their samples, at their own rate.
    pub decoded_files: usize,
    pub sample_bytes: usize,
    pub peak_bytes: usize,
}

impl AudioMemory {
    pub fn add(&mut self, data: &AudioData) {
        self.files += 1;
        self.peak_bytes += data.peaks().byte_size();
        if let Some(rhythm) = data.rhythm() {
            self.peak_bytes += rhythm.onsets.byte_size();
        }
        if let Some(samples) = data.samples() {
            self.decoded_files += 1;
            self.sample_bytes += samples.byte_size();
        }
    }
}

/// The audio files clips use, and the copies the engine plays.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct SourceMemory {
    pub files: AudioMemory,
    /// Copies converted to the engine's rate; files already at that rate
    /// share their decoded samples, and aren't counted again.
    pub engine_copies: usize,
    pub engine_bytes: usize,
}
