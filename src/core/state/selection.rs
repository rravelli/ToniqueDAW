//! Clip selection: the selected clips, and the zone (a time range over a
//! span of tracks) they were picked with, if any.

use super::{ProjectState, clip_ops::ClipOp};
use crate::core::clip::AudioClip;
use tonique_engine::{edit::ClipId, time::BeatPos};

/// How far a clip edge may be past a zone edge and still count as on it, in
/// beats: trims don't land exactly.
const EDGE: f64 = 1e-4;

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

#[derive(Debug, Clone, Default)]
pub struct ClipSelection {
    clips: Vec<ClipId>,
    bounds: Option<SelectionBounds>,
}

impl ProjectState {
    pub fn selected_clips(&self) -> &[ClipId] {
        &self.clip_selection.clips
    }
    pub fn is_clip_selected(&self, id: ClipId) -> bool {
        self.clip_selection.clips.contains(&id)
    }
    /// The zone the clips were selected with, if any.
    pub fn selection_bounds(&self) -> Option<SelectionBounds> {
        self.clip_selection.bounds
    }
    /// Select exactly these clips.
    pub fn select_clips(&mut self, ids: Vec<ClipId>) {
        self.clip_selection = ClipSelection {
            clips: ids,
            bounds: None,
        };
    }
    /// Add the clip to the selection, or remove it if it's already in.
    pub fn toggle_clip_selected(&mut self, id: ClipId) {
        let selection = &mut self.clip_selection;
        selection.bounds = None;
        if let Some(i) = selection.clips.iter().position(|c| *c == id) {
            selection.clips.remove(i);
        } else {
            selection.clips.push(id);
        }
    }
    pub fn select_all_clips(&mut self) {
        let clips = self.tracks().flat_map(|t| t.clips).map(|c| c.id).collect();
        self.select_clips(clips);
    }
    pub fn clear_clip_selection(&mut self) {
        self.clip_selection = ClipSelection::default();
    }
    /// Select the clips overlapping `bounds`, and keep the zone.
    pub fn select_in_bounds(&mut self, bounds: SelectionBounds) {
        let bpm = self.bpm();
        let tracks = bounds.start_track_index..=bounds.end_track_index;
        let clips = self
            .tracks()
            .enumerate()
            .filter(|(i, t)| tracks.contains(i) && !self.is_hidden(t.id))
            .flat_map(|(_, t)| t.clips)
            .filter(|c| bounds.overlaps(c, bpm))
            .map(|c| c.id)
            .collect();
        self.clip_selection = ClipSelection {
            clips,
            bounds: Some(bounds),
        };
    }
    /// The time range covered by the selection: its zone, or else the span
    /// of the selected clips.
    pub fn selection_range(&self) -> Option<(BeatPos, BeatPos)> {
        if let Some(b) = self.clip_selection.bounds {
            return Some((b.start_pos, b.end_pos));
        }
        let bpm = self.bpm();
        self.clip_selection
            .clips
            .iter()
            .filter_map(|id| self.find_clip(*id))
            .map(|(_, c)| (c.position, c.end(bpm)))
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
    }
    /// Delete the selected clips; with a zone, only their part inside it.
    pub fn delete_selected_clips(&mut self) {
        if self.clip_selection.bounds.is_none() {
            let ids = std::mem::take(&mut self.clip_selection).clips;
            return self.delete_clips(&ids);
        }
        self.begin_batch();
        self.split_at_zone();
        let ids = std::mem::take(&mut self.clip_selection).clips;
        self.delete_clips(&ids);
        self.commit_batch();
    }
    /// Split the selected clips at the zone's edges, so that editing them
    /// edits only what's inside it: that part keeps each clip's id and stays
    /// selected; the parts outside become new clips. Does nothing without a
    /// zone. Joins the open batch, if any.
    pub fn split_at_zone(&mut self) {
        let Some(bounds) = self.clip_selection.bounds else {
            return;
        };
        let (start, end) = (bounds.start_pos, bounds.end_pos);
        let bpm = self.bpm();
        let clips: Vec<_> = self
            .clip_selection
            .clips
            .iter()
            .filter_map(|id| self.find_clip(*id))
            .filter(|(_, clip)| bounds.overlaps(clip, bpm))
            .collect();
        self.clip_selection.clips = clips.iter().map(|(_, c)| c.id).collect();
        self.clip_ops("Split clips", |s, ops| {
            for (track, clip) in clips {
                let before = clip.position < start - EDGE;
                let after = clip.end(bpm) > end + EDGE;
                if !before && !after {
                    continue;
                }
                let mut inside = clip.clone();
                inside.crop(start, end, bpm);
                ops.push(ClipOp::Resize(track, inside));
                if before {
                    let mut left = clip.with_id(s.new_clip_id());
                    left.trim_end_at(start, bpm);
                    ops.push(ClipOp::Add(track, left));
                }
                if after {
                    let mut right = clip.with_id(s.new_clip_id());
                    right.trim_start_at(end, bpm);
                    ops.push(ClipOp::Add(track, right));
                }
            }
        });
    }
    /// Copy the selected clips right after the selection, and select the
    /// copies (with the zone moved along).
    pub fn duplicate_selected_clips(&mut self) {
        let bounds = self.clip_selection.bounds;
        let copies = self.duplicate_clips(
            &self.clip_selection.clips.clone(),
            bounds.map(|b| (b.start_pos, b.end_pos)),
        );
        self.clip_selection = ClipSelection {
            clips: copies,
            bounds: bounds.map(|b| SelectionBounds {
                start_pos: b.end_pos,
                end_pos: b.end_pos + (b.end_pos - b.start_pos),
                ..b
            }),
        };
    }
    /// Loop over the selection and start looping. Returns false if nothing
    /// is selected.
    pub fn loop_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection_range() else {
            return false;
        };
        self.set_loop_range(start, end);
        self.set_looping(true);
        true
    }
    /// Move the selected clips (and zone) `delta` tracks down (up when
    /// negative) as one block, staying within the existing tracks.
    pub fn move_selection_tracks(&mut self, delta: i32) {
        let ids = self.selected_clips().to_vec();
        let moves: Vec<_> = ids
            .iter()
            .filter_map(|id| self.find_clip(*id))
            .filter_map(|(track, clip)| Some((self.track_index(track)? as i32, clip)))
            .collect();
        let (Some(top), Some(bottom)) = (
            moves.iter().map(|(i, _)| *i).min(),
            moves.iter().map(|(i, _)| *i).max(),
        ) else {
            return;
        };
        let last = self.track_count() as i32 - 1;
        let delta = delta.clamp(-top, last - bottom);
        if delta == 0 {
            return;
        }
        let tracks: Vec<_> = self.edit().tracks.iter().map(|t| t.id).collect();
        // One undo step; the selected clips don't carve each other.
        self.begin_batch();
        // With a zone, only its part moves.
        self.split_at_zone();
        let ids = self.selected_clips().to_vec();
        let moves: Vec<_> = ids
            .iter()
            .filter_map(|id| self.find_clip(*id))
            .filter_map(|(track, clip)| Some((self.track_index(track)? as i32, clip)))
            .collect();
        for (index, clip) in &moves {
            let to = tracks[(index + delta) as usize];
            self.move_clip(&clip.id, &to, clip.position, &ids);
        }
        self.commit_batch();
        if let Some(b) = &mut self.clip_selection.bounds {
            b.start_track_index = (b.start_track_index as i32 + delta) as usize;
            b.end_track_index = (b.end_track_index as i32 + delta) as usize;
        }
    }
    /// Move the selection zone along with nudged clips.
    pub(super) fn shift_selection_bounds(&mut self, delta: f64) {
        if let Some(b) = &mut self.clip_selection.bounds {
            b.start_pos += delta;
            b.end_pos += delta;
        }
    }
    /// Forget selected clips that no longer exist (e.g. after an undo).
    pub(super) fn prune_clip_selection(&mut self) {
        let mut clips = std::mem::take(&mut self.clip_selection.clips);
        clips.retain(|id| self.find_clip(*id).is_some());
        self.clip_selection.clips = clips;
    }
}
