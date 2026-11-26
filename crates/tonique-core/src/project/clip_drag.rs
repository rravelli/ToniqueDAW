//! Clips dragged in the arrangement, the same in every UI: moved (or
//! copied) by their header, or trimmed by an edge. The UI finds what the
//! pointer is over and draws the clips where they'd land; this decides
//! where that is and what landing does, in beats and track indexes.

use tonique_engine::{
    edit::{ClipId, TrackId},
    time::BeatPos,
};

use super::ProjectState;
use crate::{document::clip::AudioClip, session::timeline_view::TimelineView};

/// The selection dragged by a clip's header: it follows the pointer,
/// snapping its nearest edge to the grid, other clips, the loop and the
/// edit cursor. With a zone, only the zone's part of the clips moves.
/// Rows past the last one are added on landing.
pub struct ClipMove {
    /// Copies, landed as new clips; the originals stay.
    pub duplicate: bool,
    pub clips: Vec<MovedClip>,
    /// The target an edge snapped to, to show it.
    pub snapped_to: Option<BeatPos>,
    /// The beat grabbed.
    grab: BeatPos,
}

pub struct MovedClip {
    /// Where it would land, cropped to the zone. Its id is the original's.
    pub clip: AudioClip,
    /// The index of the track it would land on: past the last, on a track
    /// landing adds.
    pub track_index: usize,
    /// Where it started.
    origin: BeatPos,
    /// Tracks from the grabbed clip's track.
    track_offset: i32,
}

impl ClipMove {
    /// Takes the selection, grabbed by clip `grabbed` at beat `grab`:
    /// grabbing a clip outside it (or outside its zone) selects that clip
    /// alone first.
    pub fn start(
        state: &mut ProjectState,
        grabbed: ClipId,
        grab: BeatPos,
        duplicate: bool,
    ) -> Self {
        let outside_zone = state
            .selection_bounds()
            .is_some_and(|zone| grab < zone.start_pos || grab > zone.end_pos);
        if !state.is_clip_selected(grabbed) || outside_zone {
            state.select_clips(vec![grabbed]);
        }
        let zone = state.selection_bounds();
        let bpm = state.bpm();
        let selected: Vec<(usize, AudioClip)> = state
            .tracks()
            .enumerate()
            .flat_map(|(index, track)| track.clips.into_iter().map(move |clip| (index, clip)))
            .filter(|(_, clip)| state.is_clip_selected(clip.id))
            .collect();
        let grabbed_track = selected
            .iter()
            .find(|(_, clip)| clip.id == grabbed)
            .map_or(0, |(index, _)| *index);
        let clips = selected
            .into_iter()
            .map(|(track_index, mut clip)| {
                if let Some(zone) = zone {
                    clip.crop(zone.start_pos, zone.end_pos, bpm);
                }
                MovedClip {
                    origin: clip.position,
                    clip,
                    track_offset: track_index as i32 - grabbed_track as i32,
                    track_index,
                }
            })
            .collect();
        Self {
            duplicate,
            clips,
            snapped_to: None,
            grab,
        }
    }

    /// The original clips moved.
    pub fn ids(&self) -> Vec<ClipId> {
        self.clips.iter().map(|moved| moved.clip.id).collect()
    }

    /// The clips shown where they were as the move's ghosts: none for
    /// copies, which leave their originals as they are.
    pub fn left_behind(&self) -> Vec<ClipId> {
        if self.duplicate {
            Vec::new()
        } else {
            self.ids()
        }
    }

    /// How many tracks landing would add past `track_count`.
    pub fn added_tracks(&self, track_count: usize) -> usize {
        self.clips
            .iter()
            .map(|moved| moved.track_index + 1)
            .max()
            .unwrap_or(0)
            .saturating_sub(track_count)
    }

    /// Follows the pointer at beat `pointer`, over track `pointer_track`
    /// (past the last row, below it). The nearest edge snaps: starts to the
    /// grid or the targets, ends to targets only; `alt` inverts snapping.
    /// The clips never start before the first beat, nor go above the
    /// first track.
    pub fn place(
        &mut self,
        state: &ProjectState,
        view: &TimelineView,
        pointer: BeatPos,
        pointer_track: usize,
        alt: bool,
    ) {
        let offset = pointer.0 - self.grab.0;
        let lowest_offset = self
            .clips
            .iter()
            .map(|moved| moved.track_offset)
            .min()
            .unwrap_or(0);
        let pointer_track = (pointer_track as i32).max(-lowest_offset);

        let mut delta = f64::INFINITY;
        let mut snapped_to = None;
        if state.grid().snaps(alt) {
            // Copies leave the originals in place: those are targets too.
            let moving = self.left_behind();
            let targets = state.snap_targets(&moving);
            let bpm = state.bpm();
            for moved in &self.clips {
                let start = BeatPos(moved.origin.0 + offset);
                let end = start + (moved.clip.end(bpm) - moved.clip.position);
                let start_snap = state.grid().snap_to_targets(start, &targets, view);
                let end_snap = Some(state.grid().snap_to_targets(end, &targets, view))
                    .filter(|(_, on_target)| *on_target);
                let candidates = [Some((start_snap, start)), end_snap.map(|snap| (snap, end))];
                for ((to, on_target), edge) in candidates.into_iter().flatten() {
                    if (to - edge).abs() < delta.abs() {
                        delta = to - edge;
                        snapped_to = on_target.then_some(to);
                    }
                }
            }
        }
        if delta == f64::INFINITY {
            delta = 0.;
        }
        let first = self
            .clips
            .iter()
            .map(|moved| moved.origin.0 + offset)
            .fold(f64::INFINITY, f64::min);
        if -first > delta {
            delta = -first;
            snapped_to = None;
        }
        for moved in &mut self.clips {
            moved.clip.position = BeatPos(moved.origin.0 + offset + delta);
            moved.track_index = (pointer_track + moved.track_offset).max(0) as usize;
        }
        self.snapped_to = snapped_to;
    }

    /// Lands the clips where they were placed, as one undo step: moved
    /// (with a zone, its part of them split off), or copied; tracks added
    /// for those past the last row. What landed is selected.
    pub fn land(self, state: &mut ProjectState) {
        let ids = self.ids();
        let mut tracks: Vec<TrackId> = state.tracks().map(|track| track.id).collect();
        let mut landed = Vec::new();
        state.begin_batch();
        if !self.duplicate {
            // Only the zone's part moves: it keeps the clips' ids.
            state.split_at_zone();
        }
        for moved in self.clips {
            while tracks.len() <= moved.track_index {
                tracks.push(state.add_track());
            }
            let track = tracks[moved.track_index];
            if self.duplicate {
                let copy = moved.clip.with_id(state.new_clip_id());
                landed.push(copy.id);
                state.add_clips(&track, vec![copy]);
            } else {
                landed.push(moved.clip.id);
                state.move_clip(&moved.clip.id, &track, moved.clip.position, &ids);
            }
        }
        state.commit_batch();
        state.select_clips(landed);
    }
}

/// A clip's edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipEdge {
    Start,
    End,
}

/// A clip trimmed by an edge: the edge follows the pointer, snapped to the
/// grid or other clips' edges, the loop and the edit cursor. Stretching,
/// the clip keeps its audio, played faster or slower to fit.
pub struct ClipTrim {
    pub original: AudioClip,
    pub edge: ClipEdge,
}

impl ClipTrim {
    pub fn new(original: AudioClip, edge: ClipEdge) -> Self {
        Self { original, edge }
    }

    /// The clip with its edge at beat `pointer`; `alt` inverts snapping.
    pub fn at(
        &self,
        state: &ProjectState,
        view: &TimelineView,
        pointer: BeatPos,
        stretch: bool,
        alt: bool,
    ) -> AudioClip {
        let at = if state.grid().snaps(alt) {
            let targets = state.snap_targets(&[self.original.id]);
            state.grid().snap_to_targets(pointer, &targets, view).0
        } else {
            pointer
        };
        let (mut clip, bpm) = (self.original.clone(), state.bpm());
        match (self.edge, stretch) {
            (ClipEdge::Start, false) => clip.trim_start_at(at, bpm),
            (ClipEdge::End, false) => clip.trim_end_at(at, bpm),
            (ClipEdge::Start, true) => clip.stretch_start_at(at, bpm),
            (ClipEdge::End, true) => clip.stretch_end_at(at, bpm),
        }
        clip
    }
}
