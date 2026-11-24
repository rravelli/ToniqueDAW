//! Clip selection: the selected clips, and the zone (a time range over a
//! span of tracks) they were picked with, if any.

use tonique_engine::{edit::ClipId, time::BeatPos};

use crate::document::clip::AudioClip;

/// How far a clip edge may be past a zone edge and still count as on it, in
/// beats: trims don't land exactly.
pub(crate) const EDGE: f64 = 1e-4;

/// A time range, in beats, over a span of tracks (indices, inclusive).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionBounds {
    pub start_track_index: usize,
    pub start_pos: BeatPos,
    pub end_track_index: usize,
    pub end_pos: BeatPos,
}

impl SelectionBounds {
    /// Whether some of `clip` is inside the zone's time range (touching an
    /// edge isn't enough).
    pub fn overlaps(&self, clip: &AudioClip, bpm: f32) -> bool {
        clip.position < self.end_pos - EDGE && clip.end(bpm) > self.start_pos + EDGE
    }

    /// Bounds between two corners, in any order.
    pub fn between(a: (usize, BeatPos), b: (usize, BeatPos)) -> Self {
        Self {
            start_track_index: a.0.min(b.0),
            start_pos: a.1.min(b.1),
            end_track_index: a.0.max(b.0),
            end_pos: a.1.max(b.1),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClipSelection {
    pub(crate) clips: Vec<ClipId>,
    pub(crate) bounds: Option<SelectionBounds>,
}
