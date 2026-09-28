//! Copy, cut and paste of clips, and nudging the selection.

use super::{MASTER_TRACK_ID, ProjectState};
use crate::core::clip::AudioClip;

/// Copied clips, placed relative to the copied range: time from its start,
/// tracks from the topmost copied one.
#[derive(Debug, Clone, Default)]
pub struct Clipboard {
    clips: Vec<(usize, AudioClip)>,
    /// Length of the copied range, in beats.
    length: f32,
    /// Index of the topmost copied track, to paste back there when no
    /// track is selected.
    top_track: usize,
}

impl ProjectState {
    pub fn can_paste(&self) -> bool {
        !self.clipboard.clips.is_empty()
    }

    /// Copy the selected clips (cropped to the selection zone, if any).
    /// Returns false if nothing is selected.
    pub fn copy_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection_range() else {
            return false;
        };
        let bpm = self.bpm();
        let cropped = self.selection_bounds().is_some();
        let clips: Vec<(usize, AudioClip)> = self
            .selected_clips()
            .iter()
            .filter_map(|id| self.find_clip(*id))
            .filter_map(|(track, mut clip)| {
                if cropped {
                    clip.crop(start, end, bpm);
                }
                clip.position -= start;
                Some((self.track_index(track)?, clip))
            })
            .collect();
        let Some(top_track) = clips.iter().map(|(t, _)| *t).min() else {
            return false;
        };
        self.clipboard = Clipboard {
            clips: clips.into_iter().map(|(t, c)| (t - top_track, c)).collect(),
            length: end - start,
            top_track,
        };
        true
    }

    /// Copy the selection, then remove it: the zone's part of the clips on
    /// its tracks, or else the selected clips.
    pub fn cut_selection(&mut self) {
        if !self.copy_selection() {
            return;
        }
        let Some(bounds) = self.selection_bounds() else {
            return self.delete_selected_clips();
        };
        let tracks: Vec<_> = self
            .tracks()
            .skip(bounds.start_track_index)
            .take(bounds.end_track_index + 1 - bounds.start_track_index)
            .map(|t| t.id)
            .collect();
        self.clip_ops("Cut clips", |s, ops| {
            for track in tracks {
                s.track_clips(track, ops)
                    .carve(bounds.start_pos, bounds.end_pos, &[], &mut || {
                        s.new_clip_id()
                    });
            }
        });
        self.clear_clip_selection();
    }

    /// Paste at the edit cursor, from the selected track down (or on the
    /// tracks the clips were copied from), creating tracks as needed. The
    /// pasted clips get selected and the edit cursor moves past them, so
    /// pasting again appends.
    pub fn paste(&mut self) {
        if !self.can_paste() {
            return;
        }
        let top_track = self
            .selected_tracks
            .first()
            .filter(|id| **id != MASTER_TRACK_ID)
            .and_then(|id| self.track_index(*id))
            .unwrap_or(self.clipboard.top_track);
        let at = self.edit_cursor;
        let clipboard = self.clipboard.clone();
        let mut pasted = Vec::new();
        // One undo step; each add sees the previous ones.
        self.begin_batch();
        for (offset, clip) in clipboard.clips {
            let index = top_track + offset;
            while self.track_count() <= index {
                self.add_track();
            }
            let track = self.edit().tracks[index].id;
            let mut copy = clip.with_id(self.new_clip_id());
            copy.position += at;
            pasted.push(copy.id);
            self.add_clips(&track, vec![copy]);
        }
        self.commit_batch();
        self.select_clips(pasted);
        self.set_edit_cursor(at + clipboard.length);
    }

    /// Move the selected clips (and zone) by `delta` beats as one block,
    /// without going before the start of the arrangement. With a zone, only
    /// its part of the clips moves.
    pub fn nudge_selection(&mut self, delta: f32) {
        let zone_start = self.selection_bounds().map(|b| b.start_pos);
        let Some(first) = self
            .selected_clips()
            .iter()
            .filter_map(|id| self.find_clip(*id))
            // Where each moved part starts: inside the zone, if any.
            .map(|(_, c)| zone_start.map_or(c.position, |start| c.position.max(start)))
            .reduce(f32::min)
        else {
            return;
        };
        let delta = delta.max(-first);
        if delta == 0. {
            return;
        }
        // One undo step; the selected clips don't carve each other.
        self.begin_batch();
        self.split_at_zone();
        let ids = self.selected_clips().to_vec();
        let moves: Vec<_> = ids.iter().filter_map(|id| self.find_clip(*id)).collect();
        for (track, clip) in &moves {
            self.move_clip(&clip.id, track, clip.position + delta, &ids);
        }
        self.commit_batch();
        self.shift_selection_bounds(delta);
    }
}
