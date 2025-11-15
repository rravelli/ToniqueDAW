use std::collections::HashMap;

use crate::audio::track::{TrackBackend, TrackKind};

pub fn build_layers(root: &str, tracks: &HashMap<String, TrackBackend>) -> Vec<Vec<String>> {
    let mut layers = Vec::new();
    let mut current = vec![root.to_string()];

    while !current.is_empty() {
        layers.push(current.clone());

        let mut next = Vec::new();
        for id in &current {
            if let Some(track) = tracks.get(id)
                && let TrackKind::Bus(data) = &track.kind
            {
                next.extend(data.children.clone());
            }
        }

        current = next;
    }

    layers.reverse(); // so children (leaves) come first
    layers
}
