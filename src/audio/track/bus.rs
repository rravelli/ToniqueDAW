use std::collections::HashMap;

use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};

use crate::audio::track::{Processor, TrackBackend};

#[derive(Clone)]
pub struct BusTrackData {
    pub children: HashMap<String, TrackBackend>,
}
impl BusTrackData {
    pub fn new() -> Self {
        Self {
            children: HashMap::new(),
        }
    }
}

impl Processor for BusTrackData {
    fn process(&mut self, pos: usize, num_frames: usize, sample_rate: usize, mix: &mut Vec<f32>) {
        self.children.par_iter_mut().for_each(|(_, track)| {
            track.process(pos, num_frames, sample_rate);
        });

        for track in self.children.values() {
            for i in 0..mix.len() {
                mix[i] += track.mix[i];
            }
        }
    }
}
