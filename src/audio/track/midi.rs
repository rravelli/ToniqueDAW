use fundsp::hacker::AudioUnit;

use crate::audio::{clip::midi::MidiClip, track::Processor};

#[derive(Clone)]
pub struct MidiTrackData {
    pub instrument: Box<dyn AudioUnit>,
    pub clips: Vec<MidiClip>,
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
    fn process(&mut self, pos: usize, num_frames: usize, sample_rate: usize, mix: &mut Vec<f32>) {}
}
