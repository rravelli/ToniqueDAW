mod action;
mod clip;
mod editor;
mod export;
mod history;
mod looping;
mod services;
mod session;
#[cfg(test)]
mod tests;
mod transport;
use crate::{
    core::{
        export::ExportStatus,
        grid::GridService,
        message::{AudioToGuiRx, GuiToAudioTx, ProcessToGuiMsg},
        metrics::GlobalMetrics,
        state::{
            action::{
                AddTrackAction, DeleteTrackAction, DuplicateTrackAction, ProjectStateAction,
                SetMutableTrackAction, SetVolumeAction,
            },
            editor::EditorState,
            history::HistoryState,
            services::track::TrackService,
            transport::TransportState,
        },
        track::{MutableTrackCore, TrackCore, TrackReferenceCore},
    },
    ui::{effect::UIEffect, effects::EffectId},
};

pub use looping::LoopState;
use std::mem::take;

#[derive(Clone, Debug)]
enum ProjectStatePendingAction {
    DeleteTrack { id: String },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaybackState {
    Paused,
    Playing,
}

pub struct ToniqueProjectState {
    export_status: ExportStatus,
    pub metrics: GlobalMetrics,
    // Services
    track_service: TrackService,
    // Pending
    pending_actions: Vec<ProjectStatePendingAction>,

    tx: GuiToAudioTx,
    rx: AudioToGuiRx,
    // Grid
    pub grid: GridService,
    loop_state: LoopState,

    ouptput_device: Option<String>,
    transport: TransportState,
    /// Ui related state
    pub editor: EditorState,
    /// history (undo, redo) state
    history: HistoryState,
}

impl ToniqueProjectState {
    pub fn new(tx: GuiToAudioTx, rx: AudioToGuiRx) -> Self {
        Self {
            metrics: GlobalMetrics::new(),
            track_service: TrackService::new(),
            pending_actions: Vec::new(),
            tx,
            rx,
            loop_state: LoopState::new(),
            export_status: ExportStatus::DONE,
            ouptput_device: None,
            transport: TransportState::new(),
            editor: EditorState::new(),
            history: HistoryState::new(),
            grid: GridService::new(),
        }
    }
    /// Update each frame the state
    pub fn update(&mut self, delta: f32) {
        self.handle_pending_actions();
        self.handle_messages();
        self.transport.update(delta);
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
    /// Move track to position `new_index`
    pub fn move_track(&mut self, id: &str, new_index: usize) {
        self.track_service.move_track(id, new_index);
    }
    /// Delete a track
    pub fn delete_track(&mut self, id: &String) {
        self.pending_actions
            .push(ProjectStatePendingAction::DeleteTrack { id: id.clone() });
    }
    /// Close or open all tracks
    pub fn set_all_close(&mut self, close: bool) {
        self.track_service.set_all_close(close);
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
    /// Apply a `ProjectStateAction` and adds it to the stack
    fn apply_action(&mut self, mut action: Box<dyn ProjectStateAction>) {
        if self.history.batching {
            self.history.batch_buffer.push(action);
            return;
        }
        if cfg!(debug_assertions) {
            println!("Applying {}", action.name());
        }
        action.apply(self);
        self.history.undo_stack.push(action);
        self.history.redo_stack.clear();
    }

    // Handle messages received from the audio thread
    fn handle_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                ProcessToGuiMsg::PlaybackPos(pos) => {
                    self.transport.playback_position = pos;
                    self.transport.playback_state = PlaybackState::Playing;
                }
                ProcessToGuiMsg::Metrics(metrics) => self.metrics = metrics,
                ProcessToGuiMsg::PreviewPos(pos) => self.transport.preview_position = pos,
                ProcessToGuiMsg::ExportUpdate(status) => self.export_status = status,
                ProcessToGuiMsg::DeviceChanged(name) => self.ouptput_device = name,
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
