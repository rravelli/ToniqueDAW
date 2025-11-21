use crate::{
    audio::track::{TrackBackend, TrackKind},
    core::metrics::GlobalMetrics,
};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::collections::{HashMap, HashSet};

pub const MASTER_TRACK_ID: &str = "master";

#[derive(Clone)]
pub struct Engine {
    pub sample_rate: usize,
    pub bpm: f32,
    pub tracks: HashMap<String, TrackBackend>,
    pub solo_tracks: Vec<String>,
    tree_layers: Vec<HashSet<String>>,
}

impl Engine {
    pub fn new(sample_rate: usize, bpm: f32) -> Self {
        let mut tracks = HashMap::new();
        tracks.insert(
            MASTER_TRACK_ID.to_string(),
            TrackBackend::new_bus(MASTER_TRACK_ID),
        );
        let mut master_set = HashSet::new();
        master_set.insert(MASTER_TRACK_ID.to_string());

        Self {
            sample_rate,
            bpm,
            tracks,
            solo_tracks: Vec::new(),
            tree_layers: vec![master_set],
        }
    }

    pub fn process(&mut self, pos: usize, num_frames: usize, metrics: &mut GlobalMetrics) {
        for layer in self.tree_layers.as_slice() {
            let child_map: HashMap<_, _> = layer
                .iter()
                .filter_map(|id| {
                    self.tracks
                        .get(id)
                        .map(|t| (id.clone(), t.collect_children(&self.tracks)))
                })
                .collect();

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
        self.tree_layers = build_layers(MASTER_TRACK_ID, &self.tracks);
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
        self.tree_layers = build_layers(MASTER_TRACK_ID, &self.tracks);
    }
}

pub fn build_layers(root: &str, tracks: &HashMap<String, TrackBackend>) -> Vec<HashSet<String>> {
    let mut layers = Vec::new();
    let mut current = HashSet::new();
    current.insert(root.to_string());

    while !current.is_empty() {
        layers.push(current.clone());

        let mut next = HashSet::new();
        for id in &current {
            if let Some(track) = tracks.get(id)
                && let TrackKind::Bus(data) = &track.kind
            {
                next.extend(data.children.clone());
            }
        }

        current = next;
    }

    layers.reverse();
    layers
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_new() {
        let engine = Engine::new(48000, 133.1);

        assert_eq!(engine.bpm, 133.1);
        assert_eq!(engine.sample_rate, 48000);
        assert!(engine.tracks.contains_key(MASTER_TRACK_ID));
        let mut expected = HashSet::new();
        expected.insert(MASTER_TRACK_ID.to_string());
        assert_eq!(engine.tree_layers, vec![expected]);
    }

    #[test]
    fn test_add_track() {
        let mut engine = Engine::new(48000, 133.1);
        let track = TrackBackend::new_audio_track("test_track");
        engine.add_track(track.clone(), None);

        assert!(engine.tree_layers[0].contains(&track.id));
        assert!(engine.tracks.contains_key(&track.id))
    }

    #[test]
    fn test_remove_track() {
        let mut engine = Engine::new(48000, 133.1);
        let track = TrackBackend::new_audio_track("test_track");
        engine.add_track(track.clone(), None);
        engine.solo_tracks.push(track.id.clone());

        engine.remove_track(&track.id);

        assert!(!engine.tree_layers[0].contains(&track.id));
        assert!(!engine.tracks.contains_key(&track.id));
        assert!(!engine.solo_tracks.contains(&track.id));
    }
}
