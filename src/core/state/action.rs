use crate::core::{
    clip::ClipCore,
    state::ToniqueProjectState,
    track::{MutableTrackCore, TrackCore},
};
use std::collections::HashMap;

/// Action that can be undone in the project state.
pub trait ProjectStateAction {
    fn apply(&mut self, state: &mut ToniqueProjectState);
    fn undo(&mut self, state: &mut ToniqueProjectState);
    fn name(&self) -> &str;
}

/// Group multiple action together so that they can be undone together
pub struct BatchAction {
    actions: Vec<Box<dyn ProjectStateAction>>,
}

impl BatchAction {
    pub fn new(actions: Vec<Box<dyn ProjectStateAction>>) -> Self {
        Self { actions: actions }
    }
}

impl ProjectStateAction for BatchAction {
    fn apply(&mut self, state: &mut ToniqueProjectState) {
        for action in self.actions.iter_mut() {
            action.apply(state);
        }
    }
    fn undo(&mut self, state: &mut ToniqueProjectState) {
        for action in self.actions.iter_mut().rev() {
            action.undo(state);
        }
    }
    fn name(&self) -> &str {
        "Batch action"
    }
}

// Track related actions
pub struct AddTrackAction {
    track: TrackCore,
    index: usize,
}

impl AddTrackAction {
    pub fn new(track: TrackCore, index: usize) -> Self {
        Self { track, index }
    }
}

impl ProjectStateAction for AddTrackAction {
    fn apply(&mut self, state: &mut ToniqueProjectState) {
        state
            .track_service
            .insert(self.track.clone(), self.index, &mut state.tx);
    }
    fn undo(&mut self, state: &mut ToniqueProjectState) {
        state
            .track_service
            .delete(&self.track.id.to_string(), &mut state.tx);
    }
    fn name(&self) -> &str {
        "Add Track"
    }
}

pub struct DeleteTrackAction {
    id: String,
    deleted_track: Option<(TrackCore, usize)>,
}

impl DeleteTrackAction {
    pub fn new(id: &String) -> Self {
        Self {
            id: id.clone(),
            deleted_track: None,
        }
    }
}

impl ProjectStateAction for DeleteTrackAction {
    fn apply(&mut self, state: &mut ToniqueProjectState) {
        self.deleted_track = state.track_service.delete(&self.id, &mut state.tx);
    }
    fn undo(&mut self, state: &mut ToniqueProjectState) {
        if let Some((track, index)) = self.deleted_track.clone() {
            state.track_service.insert(track, index, &mut state.tx);
        }
    }
    fn name(&self) -> &str {
        "Delete Track"
    }
}

pub struct DuplicateTrackAction {
    id: String,
    duplicated_track: Option<String>,
}

impl DuplicateTrackAction {
    pub fn new(id: &String) -> Self {
        Self {
            id: id.to_string(),
            duplicated_track: None,
        }
    }
}

impl ProjectStateAction for DuplicateTrackAction {
    fn apply(&mut self, state: &mut ToniqueProjectState) {
        self.duplicated_track = state.track_service.duplicate(&self.id, &mut state.tx);
    }
    fn undo(&mut self, state: &mut ToniqueProjectState) {
        if let Some(new_id) = self.duplicated_track.clone() {
            state.track_service.delete(&new_id, &mut state.tx);
        }
    }
    fn name(&self) -> &str {
        "Duplicate Track"
    }
}

pub struct SetVolumeAction {
    track: String,
    old_volume: f32,
    new_volume: f32,
}

impl SetVolumeAction {
    pub fn new(track: String, old_volume: f32, new_volume: f32) -> Self {
        Self {
            track,
            old_volume,
            new_volume,
        }
    }
}

impl ProjectStateAction for SetVolumeAction {
    fn apply(&mut self, state: &mut ToniqueProjectState) {
        state
            .track_service
            .set_volume(&self.track, self.new_volume, &mut state.tx);
    }
    fn undo(&mut self, state: &mut ToniqueProjectState) {
        state
            .track_service
            .set_volume(&self.track, self.old_volume, &mut state.tx);
    }
    fn name(&self) -> &str {
        "Set Track volume"
    }
}

pub struct SetMutableTrackAction {
    track: String,
    old: MutableTrackCore,
    new: MutableTrackCore,
}

impl SetMutableTrackAction {
    pub fn new(track: &String, old: MutableTrackCore, new: MutableTrackCore) -> Self {
        Self {
            track: track.to_string(),
            old,
            new,
        }
    }
}

impl ProjectStateAction for SetMutableTrackAction {
    fn apply(&mut self, state: &mut ToniqueProjectState) {
        if let Some(track) = state.track_service.get(&self.track) {
            track.mutable = self.new.clone();
            track.old_mutable = self.new.clone();
        }
    }
    fn undo(&mut self, state: &mut ToniqueProjectState) {
        if let Some(track) = state.track_service.get(&self.track) {
            track.mutable = self.old.clone();
            track.old_mutable = self.old.clone();
        }
    }
    fn name(&self) -> &str {
        "Change track"
    }
}

// Clip related actions
pub struct AddClipsAction {
    track_id: String,
    clips: Vec<ClipCore>,
    added_clips: Vec<ClipCore>,
    deleted_clips: Vec<ClipCore>,
}

impl AddClipsAction {
    pub fn new(clips: Vec<ClipCore>, track_id: &String) -> Self {
        Self {
            clips,
            track_id: track_id.clone(),
            added_clips: Vec::new(),
            deleted_clips: Vec::new(),
        }
    }
}

impl ProjectStateAction for AddClipsAction {
    fn apply(&mut self, state: &mut ToniqueProjectState) {
        if let Some(track) = state.track_service.get(&self.track_id) {
            (self.added_clips, self.deleted_clips) =
                track.add_clips(&self.clips, state.bpm, &mut state.tx);
        }
    }
    fn undo(&mut self, state: &mut ToniqueProjectState) {
        if let Some(track) = state.track_service.get(&self.track_id) {
            if self.added_clips.len() > 0 {
                track.delete_clips(
                    &self.added_clips.iter().map(|c| c.id.clone()).collect(),
                    &mut state.tx,
                );
            }
            if self.deleted_clips.len() > 0 {
                track.add_clips_skip_overlap_check(self.deleted_clips.clone(), &mut state.tx);
            }
        }
    }
    fn name(&self) -> &str {
        "Add clips"
    }
}

pub struct DeleteClipsAction {
    ids: Vec<String>,
    deleted_clips: HashMap<String, Vec<ClipCore>>,
}

impl DeleteClipsAction {
    pub fn new(ids: &Vec<String>) -> Self {
        Self {
            ids: ids.clone(),
            deleted_clips: HashMap::new(),
        }
    }
}

impl ProjectStateAction for DeleteClipsAction {
    fn apply(&mut self, state: &mut ToniqueProjectState) {
        self.deleted_clips = state.track_service.delete_clips(&self.ids, &mut state.tx);
    }
    fn undo(&mut self, state: &mut ToniqueProjectState) {
        if self.deleted_clips.len() > 0 {
            state
                .track_service
                .add_clips_skip_overlap_check(self.deleted_clips.clone(), &mut state.tx);
        }
    }
    fn name(&self) -> &str {
        "Delete clips"
    }
}

pub struct MoveClipAction {
    id: String,
    to_track: String,
    to_pos: f32,
    ignore: Vec<String>,
    from: Option<(String, f32)>,
    added_clips: Vec<ClipCore>,
    deleted_clips: Vec<ClipCore>,
}
// TODO: Implement moving multiple clips
impl MoveClipAction {
    pub fn new(id: &String, to_track: &String, to_pos: f32, ignore: &Vec<String>) -> Self {
        Self {
            id: id.to_string(),
            to_track: to_track.to_string(),
            to_pos,
            from: None,
            added_clips: Vec::new(),
            deleted_clips: Vec::new(),
            ignore: ignore.clone(),
        }
    }
}

impl ProjectStateAction for MoveClipAction {
    fn apply(&mut self, state: &mut ToniqueProjectState) {
        if let Some(track) = state.track_service._track_from_clip_id(&self.id) {
            if let Some(clip) = track.clips.iter().find(|c| c.id == self.id) {
                self.from = Some((track.id.to_string(), clip.position));
            }
        }

        (self.added_clips, self.deleted_clips) = state.track_service.move_clip(
            &self.id,
            &self.to_track,
            self.to_pos,
            state.bpm,
            &self.ignore,
            &mut state.tx,
        );
    }
    fn undo(&mut self, state: &mut ToniqueProjectState) {
        if let Some((from_track, from_pos)) = self.from.clone() {
            let added_clip_ids: Vec<String> =
                self.added_clips.iter().map(|c| c.id.clone()).collect();

            if let Some(track) = state.track_service.get(&self.to_track) {
                // Delete and re-add clips within the same track
                if self.deleted_clips.len() > 0 {
                    track.delete_clips(&added_clip_ids, &mut state.tx);
                }
                if self.deleted_clips.len() > 0 {
                    track.add_clips_skip_overlap_check(self.deleted_clips.clone(), &mut state.tx);
                }
            }

            state.track_service.move_clip_skip_overlap_check(
                &self.id,
                &from_track,
                from_pos,
                &mut state.tx,
            );
        }
    }
    fn name(&self) -> &str {
        "Move clips"
    }
}

pub struct CutClipAction {
    track: String,
    at: f32,
    previous: Option<(ClipCore, ClipCore, ClipCore)>, // Original Clip, Resized_Clip, Added Clip
}

impl CutClipAction {
    pub fn new(track: &String, at: f32) -> Self {
        Self {
            track: track.to_string(),
            at,
            previous: None,
        }
    }
}

impl ProjectStateAction for CutClipAction {
    fn apply(&mut self, state: &mut ToniqueProjectState) {
        if let Some(track) = state.track_service.get(&self.track) {
            self.previous = track.cut_clip_at(self.at, state.bpm, &mut state.tx);
        }
    }
    fn undo(&mut self, state: &mut ToniqueProjectState) {
        if let Some((original, _, added)) = self.previous.clone() {
            if let Some(track) = state.track_service.get(&self.track) {
                track.delete_clips(&vec![added.id.clone()], &mut state.tx);
                track.resize_clip_skip_overlap_check(
                    &original.id,
                    original.trim_start,
                    original.trim_end,
                    original.position,
                    &mut state.tx,
                );
            }
        }
    }
    fn name(&self) -> &str {
        "Cut clip"
    }
}

pub struct DuplicateClipAction {
    ids: Vec<String>,
    bounds: Option<(f32, f32)>,
    added_clips: HashMap<String, Vec<ClipCore>>,
    deleted_clips: HashMap<String, Vec<ClipCore>>,
}
// TODO: Fix clips not selected when duplicated
impl DuplicateClipAction {
    pub fn new(ids: &Vec<String>, bounds: Option<(f32, f32)>) -> Self {
        Self {
            ids: ids.clone(),
            bounds,
            added_clips: HashMap::new(),
            deleted_clips: HashMap::new(),
        }
    }
}

impl ProjectStateAction for DuplicateClipAction {
    fn apply(&mut self, state: &mut ToniqueProjectState) {
        let mut applied = false;
        if !self.deleted_clips.is_empty() {
            state.track_service.delete_clips(
                &self
                    .deleted_clips
                    .values()
                    .flatten()
                    .map(|c| c.id.clone())
                    .collect(),
                &mut state.tx,
            );
            applied = true;
        }
        if !self.added_clips.is_empty() {
            for (track_id, clips) in &self.added_clips {
                if let Some(track) = state.track_service.get(track_id) {
                    track.add_clips_skip_overlap_check(clips.clone(), &mut state.tx);
                }
            }
            applied = true;
        }

        if !applied {
            (self.added_clips, self.deleted_clips) = state.track_service.duplicate_clips(
                &self.ids,
                self.bounds,
                state.bpm,
                &mut state.tx,
            );
        }
    }
    fn undo(&mut self, state: &mut ToniqueProjectState) {
        state.track_service.delete_clips(
            &self
                .added_clips
                .values()
                .flatten()
                .map(|c| c.id.clone())
                .collect(),
            &mut state.tx,
        );
        for (track_id, clips) in &self.deleted_clips {
            if let Some(track) = state.track_service.get(track_id) {
                track.add_clips_skip_overlap_check(clips.clone(), &mut state.tx);
            }
        }
    }
    fn name(&self) -> &str {
        "Duplicate clips"
    }
}

pub struct ResizeClipAction {
    id: String,
    start: f32,
    end: f32,
    pos: f32,
    updated: Option<(
        ClipCore,
        TrackCore,
        HashMap<String, Vec<ClipCore>>,
        Vec<ClipCore>,
    )>,
}

impl ResizeClipAction {
    pub fn new(id: &str, start: f32, end: f32, pos: f32) -> Self {
        Self {
            id: id.to_string(),
            start,
            end,
            pos,
            updated: None,
        }
    }
}

impl ProjectStateAction for ResizeClipAction {
    fn apply(&mut self, state: &mut ToniqueProjectState) {
        self.updated = state.track_service.resize_clip(
            &self.id,
            self.start,
            self.end,
            self.pos,
            state.bpm,
            &mut state.tx,
        );
    }
    fn undo(&mut self, state: &mut ToniqueProjectState) {
        if let Some((original_clip, track, added_clips, deleted_clips)) = self.updated.clone() {
            state.track_service.resize_clip_skip_overlap_check(
                &original_clip.id,
                original_clip.trim_start,
                original_clip.trim_end,
                original_clip.position,
                &mut state.tx,
            );
            if !deleted_clips.is_empty() {
                let mut map = HashMap::new();
                map.insert(track.id, deleted_clips);
                state
                    .track_service
                    .add_clips_skip_overlap_check(map, &mut state.tx);
            }
            if !added_clips.is_empty() {
                state.track_service.delete_clips(
                    &added_clips
                        .values()
                        .flatten()
                        .map(|c| c.id.to_string())
                        .collect(),
                    &mut state.tx,
                );
            }
        }
    }
    fn name(&self) -> &str {
        "Resize clip"
    }
}
