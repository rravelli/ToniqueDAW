use crate::{
    audio::{process::build_layers, track::TrackBackend},
    core::{metrics::GlobalMetrics, state::LoopState},
};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::collections::HashMap;

#[derive(Clone)]
pub struct Engine {
    pub sample_rate: usize,
    pub bpm: f32,
    pub tracks: HashMap<String, TrackBackend>,
    pub solo_tracks: Vec<String>,
    pub loop_state: LoopState,
}

impl Engine {
    pub fn new(sample_rate: usize, bpm: f32) -> Self {
        let mut tracks = HashMap::new();
        tracks.insert("master".to_string(), TrackBackend::new_bus("master"));

        Self {
            sample_rate,
            bpm,
            tracks,
            solo_tracks: Vec::new(),
            loop_state: LoopState {
                enabled: false,
                start: 0.,
                end: 40.,
            },
        }
    }

    pub fn process(&mut self, pos: usize, num_frames: usize, metrics: &mut GlobalMetrics) {
        let layers = build_layers("master", &self.tracks);

        for layer in layers {
            let mut child_map = HashMap::new();
            for id in &layer {
                if let Some(track) = self.tracks.get(id) {
                    child_map.insert(id.clone(), track.collect_children(&self.tracks));
                }
            }

            let layer_tracks: Vec<_> = self
                .tracks
                .values_mut()
                .filter(|t| layer.contains(&t.id))
                .map(|t| (child_map.get(&t.id).unwrap_or(&vec![]).clone(), t))
                .collect();

            // Process layer in parallel
            layer_tracks.into_par_iter().for_each(|(children, track)| {
                track.process(
                    pos,
                    num_frames,
                    self.sample_rate,
                    self.bpm,
                    children,
                    &self.solo_tracks,
                );
            });
        }

        // Collect tracks
        // let tracks: Vec<_> = self.tracks.values_mut().collect();

        // // Process all tracks in parallel
        // tracks.into_par_iter().for_each(|track| {
        //     let mut track_metrics = AudioMetrics::new();

        //     track.process(pos, num_frames, self.sample_rate, self.bpm);

        //     for i in 0..track.mix.len() {
        //         track_metrics.add_sample(track.mix[i], (i % 2 == 0).into());
        //     }
        // });

        // Add processed tracks to global mix
        for track in self.tracks.values() {
            metrics
                .tracks
                .insert(track.id.clone(), track.metrics.clone());
        }
    }
}
