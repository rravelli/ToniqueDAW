use crate::audio::{
    clip::midi::MidiClip,
    track::{Processor, TrackBackend},
};
use fundsp::hacker::AudioUnit;
use std::{collections::HashMap, fmt::Debug};

#[derive(Clone)]
pub struct MidiTrackData {
    pub instrument: Box<dyn AudioUnit>,
    pub clips: Vec<MidiClip>,
}

impl Debug for MidiTrackData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MidiTrackData").finish()
    }
}

impl MidiTrackData {
    fn new(instrument: Box<dyn AudioUnit>) -> Self {
        Self {
            clips: Vec::new(),
            instrument,
        }
    }
}

impl Processor for MidiTrackData {
    fn process(
        &mut self,
        pos: usize,
        num_frames: usize,
        sample_rate: usize,
        mix: &mut Vec<f32>,
        bpm: f32,
    ) {
    }
}
