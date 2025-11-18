use crate::{
    audio::{
        process::build_layers,
        track::{TrackBackend, TrackKind},
    },
    core::{metrics::GlobalMetrics, state::LoopState},
};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::collections::HashMap;

pub const MASTER_TRACK_ID: &str = "master";

#[derive(Clone)]
pub struct Engine {
    pub sample_rate: usize,
    pub bpm: f32,
    pub tracks: HashMap<String, TrackBackend>,
    pub solo_tracks: Vec<String>,
    _tree_layers: Vec<Vec<String>>,
}

impl Engine {
    pub fn new(sample_rate: usize, bpm: f32) -> Self {
        let mut tracks = HashMap::new();
        tracks.insert(
            MASTER_TRACK_ID.to_string(),
            TrackBackend::new_bus(MASTER_TRACK_ID),
        );

        Self {
            sample_rate,
            bpm,
            tracks,
            solo_tracks: Vec::new(),
            _tree_layers: vec![vec![MASTER_TRACK_ID.to_string()]],
        }
    }

    pub fn process(&mut self, pos: usize, num_frames: usize, metrics: &mut GlobalMetrics) {
        for layer in self._tree_layers.as_slice() {
            let mut child_map = HashMap::new();
            for id in layer {
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

        // Add processed tracks to global mix
        for track in self.tracks.values() {
            metrics
                .tracks
                .insert(track.id.clone(), track.metrics.clone());
        }
    }

    pub fn add_track(&mut self, track: TrackBackend, parent: Option<&str>) {
        let parent_id = parent.unwrap_or(MASTER_TRACK_ID);
        if let Some(parent_track) = self.tracks.get_mut(parent_id)
            && let TrackKind::Bus(data) = &mut parent_track.kind
        {
            data.children.push(track.id.clone());
        } else {
            return;
        }
        self.tracks.insert(track.id.clone(), track);
        // Recompute tree
        self._tree_layers = build_layers(MASTER_TRACK_ID, &self.tracks);
    }

    pub fn remove_track(&mut self, id: &str) {
        self.tracks.remove(id);
        // Remove from parent children
        if let Some(track) = self.tracks.get_mut(MASTER_TRACK_ID)
            && let TrackKind::Bus(data) = &mut track.kind
        {
            data.children.retain(|cid| *cid != id);
        }
        self.solo_tracks.retain(|solo| *solo != *id);
        // Recompute tree
        self._tree_layers = build_layers(MASTER_TRACK_ID, &self.tracks);
    }
}
