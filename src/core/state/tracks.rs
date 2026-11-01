//! Adding, duplicating, deleting and folding tracks.

use crate::core::track::{TRACK_COLLAPSED_HEIGHT, TrackView};
use egui::Color32;

use tonique_engine::edit::{BusId, Output, Track, TrackId, commands::AddTrack};

use super::{
    MASTER_TRACK_ID, MIN_EXPANDED_HEIGHT, ProjectState, ProjectStatePendingAction, TRACK_NAME,
};

impl ProjectState {
    /// Add track at the last position. Shortcut for `add_track_at`
    pub fn add_track(&mut self) -> TrackId {
        self.add_track_at(self.track_count())
    }
    /// Add track at specific index
    pub fn add_track_at(&mut self, index: usize) -> TrackId {
        let mut track = self.session.create(|e| Track::new(e, TRACK_NAME));
        // Inside the group its neighbours share, so no group is split.
        if let Some(group) = self.insertion_parent(index) {
            track.output = Output::Bus(BusId(group.0));
        }
        let id = track.id;
        let mut view = TrackView::new();
        view.color = self.next_track_color();
        self.views.insert(id, view);
        self.perform(AddTrack::at(track, index));
        id
    }

    /// The palette colour the fewest tracks use, the first one on ties, so
    /// new tracks go through the palette before repeating a colour.
    pub(super) fn next_track_color(&self) -> Color32 {
        self.new_track_colors(1)
            .first()
            .copied()
            .unwrap_or(Color32::GRAY)
    }
    /// The colours the next `count` new tracks will get, in order: each the
    /// palette's least used one so far. Grey without a palette.
    pub fn new_track_colors(&self, count: usize) -> Vec<Color32> {
        let mut uses: Vec<usize> = self
            .track_palette
            .iter()
            .map(|color| {
                self.views
                    .iter()
                    .filter(|(id, view)| **id != MASTER_TRACK_ID && view.color == *color)
                    .count()
            })
            .collect();
        (0..count)
            .map(|_| {
                // The first of the least used, as `min_by_key` picks.
                let Some((index, _)) = uses.iter().enumerate().min_by_key(|(_, uses)| **uses)
                else {
                    return Color32::GRAY;
                };
                uses[index] += 1;
                self.track_palette[index]
            })
            .collect()
    }

    /// Colours for new tracks; the UI passes the installed theme's.
    pub fn set_track_palette(&mut self, palette: &[Color32]) {
        if self.track_palette != palette {
            self.track_palette = palette.to_vec();
        }
    }
    /// Duplicate track. New track is inserted after the current track.
    pub fn duplicate_track(&mut self, id: &TrackId) {
        let Ok(original) = self.edit().track(*id).cloned() else {
            return;
        };
        let index = self.track_index(*id).unwrap_or(self.track_count());
        let copy = self.session.create(|e| original.duplicate(e));
        let copy_id = copy.id;
        if let Some(view) = self.views.get(id).cloned() {
            self.views.insert(copy_id, view);
        }
        self.perform(AddTrack::at(copy, index + 1));
        self.select_track(&copy_id);
    }
    /// Delete a track
    pub fn delete_track(&mut self, id: &TrackId) {
        self.pending_actions
            .push(ProjectStatePendingAction::DeleteTrack { id: *id });
    }
    /// Collapse or expand every row.
    pub fn set_all_collapsed(&mut self, collapsed: bool) {
        let ids: Vec<TrackId> = self.views.keys().copied().collect();
        for id in ids {
            self.set_collapsed(&id, collapsed);
        }
    }
    /// Collapse or expand a row, going back to its previous height when
    /// expanded.
    pub fn set_collapsed(&mut self, id: &TrackId, collapsed: bool) {
        let view = self.track_view_mut(id);
        if view.collapsed == collapsed {
            return;
        }
        if collapsed {
            view.expanded_height = view.height;
            view.height = TRACK_COLLAPSED_HEIGHT;
        } else {
            view.height = view.expanded_height.max(MIN_EXPANDED_HEIGHT);
        }
        view.collapsed = collapsed;
    }
    /// Arm a track for recording, or disarm it. Groups and the master
    /// can't be armed.
    pub fn set_armed(&mut self, id: &TrackId, armed: bool) {
        if *id != MASTER_TRACK_ID && !self.is_group(*id) {
            self.track_view_mut(id).armed = armed;
        }
    }
}
