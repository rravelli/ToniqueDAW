mod action;
#[cfg(test)]
mod tests;
use crate::{
    core::{
        clip::ClipCore,
        message::{GuiToPlayerMsg, ProcessToGuiMsg},
        metrics::GlobalMetrics,
        services::track::TrackService,
        state::action::{
            AddClipsAction, AddTrackAction, BatchAction, CutClipAction, DeleteClipsAction,
            DeleteTrackAction, DuplicateClipAction, DuplicateTrackAction, MoveClipAction,
            ProjectStateAction, ResizeClipAction, SetMutableTrackAction, SetVolumeAction,
        },
        track::{MutableTrackCore, TrackCore, TrackReferenceCore},
    },
    ui::{effect::UIEffect, effects::EffectId, workspace::PlaybackState},
};
use rtrb::{Consumer, Producer};
use std::{mem::take, path::PathBuf};

#[derive(Clone, Debug)]
enum ProjectStatePendingAction {
    DeleteTrack { id: String },
}

pub struct ToniqueProjectState {
    bpm: f32,
    playback_position: f32,
    playback_state: PlaybackState,
    preview_playback_state: PlaybackState,
    preview_position: usize,
    pub metrics: GlobalMetrics,
    // Services
    track_service: TrackService,
    // Pending
    pending_actions: Vec<ProjectStatePendingAction>,
    // Should be private in the future
    pub tx: Producer<GuiToPlayerMsg>,
    rx: Consumer<ProcessToGuiMsg>,
    // History management
    undo_stack: Vec<Box<dyn ProjectStateAction>>,
    redo_stack: Vec<Box<dyn ProjectStateAction>>,
    batching: bool,
    batch_buffer: Vec<Box<dyn ProjectStateAction>>,
}

impl ToniqueProjectState {
    pub fn new(tx: Producer<GuiToPlayerMsg>, rx: Consumer<ProcessToGuiMsg>) -> Self {
        Self {
            bpm: 120.,
            playback_position: 0.,
            playback_state: PlaybackState::Paused,
            preview_playback_state: PlaybackState::Paused,
            preview_position: 0,
            metrics: GlobalMetrics::new(),
            track_service: TrackService::new(),
            pending_actions: Vec::new(),
            tx,
            rx,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            batching: false,
            batch_buffer: Vec::new(),
        }
    }
    /// Update each frame the state
    pub fn update(&mut self) {
        self.handle_pending_actions();
        self.handle_messages();
    }
    // Bpm
    pub fn set_bpm(&mut self, value: f32) {
        self.bpm = value;
        let _ = self.tx.push(GuiToPlayerMsg::UpdateBPM(value));
    }

    pub fn bpm(&self) -> f32 {
        self.bpm
    }
    // Playback position
    pub fn set_playback_position(&mut self, value: f32) {
        self.playback_position = value;
        let _ = self.tx.push(GuiToPlayerMsg::SeekTo(value));
    }
    pub fn playback_position(&self) -> f32 {
        self.playback_position
    }
    // Transport state
    pub fn pause(&mut self) {
        self.playback_state = PlaybackState::Paused;
        let _ = self.tx.push(GuiToPlayerMsg::Pause);
    }
    pub fn play(&mut self) {
        self.playback_state = PlaybackState::Playing;
        let _ = self.tx.push(GuiToPlayerMsg::Play);
    }
    pub fn pause_preview(&mut self) {
        self.preview_playback_state = PlaybackState::Paused;
        let _ = self.tx.push(GuiToPlayerMsg::PausePreview());
    }
    pub fn play_preview(&mut self, path: PathBuf) {
        self.preview_playback_state = PlaybackState::Playing;
        let _ = self.tx.push(GuiToPlayerMsg::PlayPreview(path));
    }
    pub fn seek_preview(&mut self, pos: usize) {
        self.preview_position = pos;
        let _ = self.tx.push(GuiToPlayerMsg::SeekPreview(pos));
    }

    pub fn playback_state(&self) -> PlaybackState {
        self.playback_state
    }
    pub fn preview_playback_state(&self) -> PlaybackState {
        self.preview_playback_state
    }
    pub fn preview_position(&self) -> usize {
        self.preview_position
    }
    // Tracks
    /// Add track at the last position. Shortcut for `add_track_at``
    pub fn add_track(&mut self, track: TrackCore) {
        self.add_track_at(track, self.track_service.length());
    }
    /// Add track at specific index
    pub fn add_track_at(&mut self, track: TrackCore, index: usize) {
        let action = AddTrackAction::new(track, index);
        self.apply_action(Box::new(action));
    }
    /// Duplicate track. New track is inserted after the current track.
    pub fn duplicate_track(&mut self, id: &String) {
        let action = DuplicateTrackAction::new(id);
        self.apply_action(Box::new(action));
    }
    /// Delete a track
    pub fn delete_track(&mut self, id: &String) {
        self.pending_actions
            .push(ProjectStatePendingAction::DeleteTrack { id: id.clone() });
    }
    // Clips
    /// Add clips and fix all overlaps on the track.
    pub fn add_clips(&mut self, track_id: &String, clips: Vec<ClipCore>) {
        let action = AddClipsAction::new(clips, track_id);
        self.apply_action(Box::new(action));
    }
    /// Move clip to a new position and a new track fixing all overlaps on this track.
    pub fn move_clip(&mut self, id: &String, to_track: &String, to_pos: f32, ignore: &Vec<String>) {
        let action = MoveClipAction::new(id, to_track, to_pos, ignore);
        self.apply_action(Box::new(action));
    }
    /// Delete clips for their ids
    pub fn delete_clips(&mut self, ids: &Vec<String>) {
        let action = DeleteClipsAction::new(ids);
        self.apply_action(Box::new(action));
    }
    /// Cut clip located at position on given track. Does nothing it there is no clip.
    pub fn cut_clip_at(&mut self, track_id: &String, position: f32) {
        let action = CutClipAction::new(track_id, position);
        self.apply_action(Box::new(action));
    }
    /// Duplicate clips fixing all overlaps on the tracks.
    pub fn duplicate_clips(&mut self, ids: &Vec<String>, bounds: Option<(f32, f32)>) {
        let action = DuplicateClipAction::new(ids, bounds);
        self.apply_action(Box::new(action));
    }
    /// Resize clip without computing overlap checks.
    /// Use `commit_resize_clip` to apply overlap checks and add to undo stack.
    pub fn resize_clip(&mut self, id: &String, start: f32, end: f32, pos: f32) {
        self.track_service
            .resize_clip_skip_overlap_check(id, start, end, pos, &mut self.tx);
    }
    /// Resize clip and perform overlap checks
    pub fn commit_resize_clip(&mut self, id: &String, start: f32, end: f32, pos: f32) {
        let action = ResizeClipAction::new(id, start, end, pos);
        self.apply_action(Box::new(action));
    }
    /// Add a effect to the track
    /// TODO: Action
    pub fn add_effect(&mut self, id: &String, effect_id: EffectId, index: usize) {
        if let Some(track) = self.track_service.get(id) {
            track.add_effect(effect_id, index, &mut self.tx);
        }
    }
    /// TODO: Action
    pub fn remove_effects(&mut self, id: &String, indexes: &Vec<usize>) {
        if let Some(track) = self.track_service.get(id) {
            track.remove_effects(indexes, &mut self.tx);
        }
    }
    /// TODO: Action
    pub fn effects_mut(&mut self, id: &String) -> Option<&mut [UIEffect]> {
        if let Some(track) = self.track_service.get(id) {
            Some(track.effects_mut())
        } else {
            None
        }
    }
    /// Set individual track volume. Changes are not saved in undo stack.
    pub fn set_volume(&mut self, id: String, volume: f32) {
        self.track_service.set_volume(&id, volume, &mut self.tx);
    }
    /// Set track volume and save in undo stack given `old_volume`.
    pub fn commit_volume(&mut self, id: String, old_volume: f32, new_volume: f32) {
        let action = SetVolumeAction::new(id, old_volume, new_volume);
        self.apply_action(Box::new(action));
    }
    /// Mute or unmute this track
    pub fn set_mute(&mut self, id: String, mute: bool) {
        self.track_service.set_mute(id, mute, &mut self.tx);
    }
    /// Toggle the solo button.
    pub fn toggle_solo(&mut self, id: String, modifier_pressed: bool) {
        self.track_service
            .toggle_solo(id, modifier_pressed, &mut self.tx);
    }
    /// Set track selected
    pub fn select_track(&mut self, id: &String) {
        self.track_service.select(&id);
    }
    ///
    pub fn deselect(&mut self) {
        self.track_service.selected_tracks.clear();
    }
    pub fn selected_track(&self) -> Option<TrackReferenceCore> {
        self.track_service.selected_track()
    }
    /// Get all tracks
    pub fn tracks(&self) -> impl Iterator<Item = TrackReferenceCore> {
        self.track_service.tracks()
    }
    pub fn master_track(&self) -> TrackReferenceCore {
        self.track_service.master_track()
    }
    pub fn selected_tracks(&self) -> &Vec<String> {
        &self.track_service.selected_tracks
    }
    pub fn track_len(&self) -> usize {
        self.track_service.length()
    }
    /// Get mutable fields from track to be changed in place. Use `self.commit_track_mut` to update the undo stack.
    pub fn track_mut(&mut self, id: &String) -> &mut MutableTrackCore {
        self.track_service.get_mut(id)
    }
    /// Commit changes made to the track mutable fields.
    pub fn commit_track_mut(&mut self, id: &String) {
        if let Some(track) = self.track_service.get(id) {
            let action =
                SetMutableTrackAction::new(id, track.old_mutable.clone(), track.mutable.clone());
            self.apply_action(Box::new(action));
        }
    }
    pub fn track_from_index(&self, index: usize) -> Option<TrackReferenceCore> {
        self.track_service.from_index(index)
    }

    // History management
    /// Apply a `ProjectStateAction` and adds it to the stack
    fn apply_action(&mut self, mut action: Box<dyn ProjectStateAction>) {
        if self.batching {
            self.batch_buffer.push(action);
            return;
        }
        if cfg!(debug_assertions) {
            println!("Applying {}", action.name());
        }
        action.apply(self);
        self.undo_stack.push(action);
        self.redo_stack.clear();
    }
    /// Create a batch of actions. All actions made from this point are not applied but saved to a buffer.
    /// Use `commit_batch` to apply them.
    pub fn begin_batch(&mut self) {
        self.batching = true;
    }
    /// Apply changes saved in the batch buffer. New actions are no longer saved in the buffer.
    pub fn commmit_batch(&mut self) {
        self.batching = false;
        let batch = std::mem::take(&mut self.batch_buffer);
        let action = BatchAction::new(batch);
        self.apply_action(Box::new(action));
        self.batch_buffer.clear();
    }
    /// Undo last action. Does nothing if there is no action.
    pub fn undo(&mut self) {
        if let Some(mut action) = self.undo_stack.pop() {
            if cfg!(debug_assertions) {
                println!("Undoing {}", action.name());
            }
            action.undo(self);
            self.redo_stack.push(action);
        }
    }
    /// Redo last action. Does nothing if there is no action.
    pub fn redo(&mut self) {
        if let Some(mut action) = self.redo_stack.pop() {
            if cfg!(debug_assertions) {
                println!("Redoing {}", action.name());
            }
            action.apply(self);
            self.undo_stack.push(action);
        }
    }
    /// Whether there is still actions to undo
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }
    /// Whether there is still actions to redo
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    // Handle messages received from the audio thread
    fn handle_messages(&mut self) {
        while let Ok(msg) = self.rx.pop() {
            match msg {
                ProcessToGuiMsg::PlaybackPos(pos) => {
                    self.playback_position = pos;
                    self.playback_state = PlaybackState::Playing;
                }
                ProcessToGuiMsg::Metrics(metrics) => self.metrics = metrics,
                ProcessToGuiMsg::PreviewPos(pos) => self.preview_position = pos,
            }
        }
    }
    /// To make sure some action do not conflict, pending actions are handled during state updates
    fn handle_pending_actions(&mut self) {
        let pendings = take(&mut self.pending_actions);
        for pending in pendings {
            match pending {
                ProjectStatePendingAction::DeleteTrack { id } => {
                    let action = DeleteTrackAction::new(&id);
                    self.apply_action(Box::new(action));
                }
            }
        }
        self.pending_actions.clear();
    }
}
