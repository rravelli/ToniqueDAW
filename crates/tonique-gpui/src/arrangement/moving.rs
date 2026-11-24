//! Clips dragged by their header, as the egui app's `ClipDrag`: the
//! selection moves with the pointer (only its zone's part, with a zone),
//! snapping its nearest edge to the grid, other clips, the loop and the
//! edit cursor; with Ctrl, copies of it do. Rows past the last one are
//! added on drop.

use crate::arrangement::Arrangement;
use crate::model::HoldsProject;
use gpui::{App, Modifiers, Pixels, Point};
use tonique_engine::{
    edit::{ClipId, TrackId},
    time::BeatPos,
};

use tonique_core::arrangement::clip::AudioClip;

/// Pixels the pointer moves before a press on a clip becomes a move.
const MOVE_THRESHOLD: f64 = 3.;

/// A press on a clip's header, until it's released.
pub struct ClipMove {
    /// The clip grabbed, and its track's index.
    pub id: ClipId,
    pub track_index: usize,
    /// Where it was pressed, in the window, and the beat there.
    pub start: Point<Pixels>,
    pub grab: BeatPos,
    /// Shift was held: the press toggled the clip in the selection, so a
    /// click doesn't select it alone.
    pub shift: bool,
    /// Once the pointer moved far enough.
    pub dragged: Option<DraggedClips>,
}

pub struct DraggedClips {
    /// Copies, dropped as new clips (Ctrl held when the drag started).
    pub duplicate: bool,
    pub clips: Vec<DraggedClip>,
    /// The clip edge's target the clips snapped to, to show it.
    pub snapped_to: Option<BeatPos>,
}

pub struct DraggedClip {
    /// Where it would land; cropped to the zone. Its id is the original's.
    pub clip: AudioClip,
    /// Where it started.
    origin: BeatPos,
    /// Tracks from the grabbed clip's track.
    track_offset: i32,
    /// The index of the track it would land on: past the last, on a track
    /// a drop adds.
    pub track_index: usize,
}

fn duplicating(modifiers: &Modifiers) -> bool {
    modifiers.control || modifiers.platform
}

impl DraggedClips {
    pub fn ids(&self) -> Vec<ClipId> {
        self.clips.iter().map(|dragged| dragged.clip.id).collect()
    }

    /// How many tracks a drop would add past `track_count`.
    pub fn added_tracks(&self, track_count: usize) -> usize {
        self.clips
            .iter()
            .map(|dragged| dragged.track_index + 1)
            .max()
            .unwrap_or(0)
            .saturating_sub(track_count)
    }
}

impl Arrangement {
    /// Follows the pointer with a press on a clip's header. Returns `None`
    /// when a copying drag lost its Ctrl: it's dropped without copying.
    pub(super) fn follow_clip_move(
        &mut self,
        mut press: ClipMove,
        position: Point<Pixels>,
        modifiers: &Modifiers,
        context: &mut App,
    ) -> Option<ClipMove> {
        if press.dragged.is_none() {
            if (position - press.start).magnitude() <= MOVE_THRESHOLD {
                return Some(press);
            }
            press.dragged = Some(self.start_clip_drag(&press, duplicating(modifiers), context));
        }
        let mut dragged = press.dragged.take()?;
        if dragged.duplicate && !duplicating(modifiers) {
            self.expand_on_hover(None, false, context);
            return None;
        }
        let (local_x, local_y) = self.local(position);
        let over_lanes = self.timeline.bounds.get().contains(&position);
        self.expand_on_hover(over_lanes.then_some(local_y), true, context);
        let offset = self.beats_at(local_x, context) - press.grab;
        self.place_dragged(&mut dragged, offset, local_y, modifiers.alt, context);
        press.dragged = Some(dragged);
        Some(press)
    }

    /// What a drag of the selection takes: grabbing a clip outside it (or
    /// outside its zone) takes that clip alone.
    fn start_clip_drag(
        &mut self,
        press: &ClipMove,
        duplicate: bool,
        context: &mut App,
    ) -> DraggedClips {
        let state = &mut *self.state_mut(context);
        let outside_zone = state
            .selection_bounds()
            .is_some_and(|zone| press.grab < zone.start_pos || press.grab > zone.end_pos);
        if !state.is_clip_selected(press.id) || outside_zone {
            state.select_clips(vec![press.id]);
        }
        let zone = state.selection_bounds();
        let bpm = state.bpm();
        let clips = state
            .tracks()
            .enumerate()
            .flat_map(|(index, track)| track.clips.into_iter().map(move |clip| (index, clip)))
            .filter(|(_, clip)| state.is_clip_selected(clip.id))
            .map(|(track_index, mut clip)| {
                if let Some(zone) = zone {
                    clip.crop(zone.start_pos, zone.end_pos, bpm);
                }
                DraggedClip {
                    origin: clip.position,
                    clip,
                    track_offset: track_index as i32 - press.track_index as i32,
                    track_index,
                }
            })
            .collect();
        DraggedClips {
            duplicate,
            clips,
            snapped_to: None,
        }
    }

    /// Where the dragged clips would land, `offset` beats from where they
    /// were grabbed, with the pointer at `local_y`.
    fn place_dragged(
        &self,
        dragged: &mut DraggedClips,
        offset: f64,
        local_y: f32,
        alt: bool,
        context: &App,
    ) {
        let rows = self.rows(context);
        let state = self.state(context);
        // Below the rows: on new tracks; the topmost clip not above the first.
        let lowest_offset = dragged
            .clips
            .iter()
            .map(|clip| clip.track_offset)
            .min()
            .unwrap_or(0);
        let pointer_track = self.pointer_track(&rows, local_y, context) as i32;
        let pointer_track = pointer_track.max(-lowest_offset);

        let mut delta = f64::INFINITY;
        let mut snapped_to = None;
        if state.grid.snaps(alt) {
            // Copies leave the originals in place: those are targets too.
            let moving = if dragged.duplicate {
                Vec::new()
            } else {
                dragged.ids()
            };
            let targets = state.snap_targets(&moving);
            let bpm = state.bpm();
            for clip in &dragged.clips {
                let start = BeatPos(clip.origin.0 + offset);
                let end = start + (clip.clip.end(bpm) - clip.clip.position);
                // Starts snap to the grid or the targets, ends to targets only.
                let start_snap = state.grid.snap_to_targets(start, &targets);
                let end_snap = Some(state.grid.snap_to_targets(end, &targets))
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
        // Not before the first beat.
        let first = dragged
            .clips
            .iter()
            .map(|clip| clip.origin.0 + offset)
            .fold(f64::INFINITY, f64::min);
        if -first > delta {
            delta = -first;
            snapped_to = None;
        }
        for clip in &mut dragged.clips {
            clip.clip.position = BeatPos(clip.origin.0 + offset + delta);
            clip.track_index = (pointer_track + clip.track_offset).max(0) as usize;
        }
        dragged.snapped_to = snapped_to;
    }

    /// Drops the dragged clips: moved (with a zone, its part of them,
    /// split off), or copied; tracks added for those past the last row.
    /// One undo step; what landed is selected.
    pub(super) fn drop_clips(&mut self, dragged: DraggedClips, context: &mut App) {
        let ids = dragged.ids();
        let state = &mut *self.state_mut(context);
        let mut tracks: Vec<TrackId> = state.tracks().map(|track| track.id).collect();
        let mut landed = Vec::new();
        state.begin_batch();
        if !dragged.duplicate {
            // Only the zone's part moves: it keeps the clips' ids.
            state.split_at_zone();
        }
        for clip in dragged.clips {
            while tracks.len() <= clip.track_index {
                tracks.push(state.add_track());
            }
            let track = tracks[clip.track_index];
            if dragged.duplicate {
                let copy = clip.clip.with_id(state.new_clip_id());
                landed.push(copy.id);
                state.add_clips(&track, vec![copy]);
            } else {
                landed.push(clip.clip.id);
                state.move_clip(&clip.clip.id, &track, clip.clip.position, &ids);
            }
        }
        state.commit_batch();
        state.select_clips(landed);
    }
}
