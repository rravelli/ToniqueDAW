use std::path::PathBuf;

use rtrb::{Consumer, Producer};

use crate::{
    core::{
        clip::ClipCore,
        services::track::TrackService,
        track::{MutableTrackCore, TrackCore, TrackReferenceCore},
    },
    message::{GuiToPlayerMsg, ProcessToGuiMsg},
    metrics::GlobalMetrics,
    ui::{effect::UIEffect, effects::EffectId, workspace::PlaybackState},
};

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
}

impl ToniqueProjectState {
    pub fn new(tx: Producer<GuiToPlayerMsg>, rx: Consumer<ProcessToGuiMsg>) -> Self {
        let mut master = TrackCore::new();
        master.mutable.name = "Master".into();
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
    /// Add track at the last position
    pub fn add_track(&mut self, track: TrackCore) {
        self.track_service
            .insert(track, self.track_service.length(), &mut self.tx);
    }
    /// Add track at specific index
    pub fn add_track_at(&mut self, track: TrackCore, index: usize) {
        self.track_service.insert(track, index, &mut self.tx);
    }
    /// Duplicate track
    pub fn duplicate_track(&mut self, id: &String) {
        self.track_service.duplicate(id, &mut self.tx);
    }
    /// Delete a track
    pub fn delete_track(&mut self, id: &String) {
        self.pending_actions
            .push(ProjectStatePendingAction::DeleteTrack { id: id.clone() });
    }
    // Clips
    pub fn add_clips(&mut self, track_id: &String, clips: Vec<ClipCore>) {
        if let Some(track) = self.track_service.get(track_id) {
            track.add_clips(&clips, self.bpm, &mut self.tx);
        }
    }
    pub fn move_clip(&mut self, id: &String, to_track: &String, to_pos: f32) {
        self.track_service
            .move_clip(id, to_track, to_pos, self.bpm, &mut self.tx);
    }
    pub fn delete_clips(&mut self, ids: &Vec<String>) {
        self.track_service.delete_clips(ids, &mut self.tx);
    }
    pub fn cut_clip_at(
        &mut self,
        track_id: &String,
        position: f32,
    ) -> Option<(ClipCore, ClipCore)> {
        if let Some(track) = self.track_service.get(track_id) {
            track.cut_clip_at(position, self.bpm, &mut self.tx)
        } else {
            None
        }
    }
    pub fn duplicate_clips(
        &mut self,
        ids: &Vec<String>,
        bounds: Option<(f32, f32)>,
    ) -> Vec<ClipCore> {
        self.track_service
            .duplicate_clips(ids, bounds, self.bpm, &mut self.tx)
    }
    pub fn resize_clip(&mut self, id: &String, start: f32, end: f32, pos: f32) {
        self.track_service
            .resize_clip(id, start, end, pos, self.bpm, &mut self.tx)
    }
    /// Add a effect to the track
    pub fn add_effect(&mut self, id: &String, effect_id: EffectId, index: usize) {
        if let Some(track) = self.track_service.get(id) {
            track.add_effect(effect_id, index, &mut self.tx);
        }
    }
    pub fn remove_effects(&mut self, id: &String, indexes: &Vec<usize>) {
        if let Some(track) = self.track_service.get(id) {
            track.remove_effects(indexes, &mut self.tx);
        }
    }
    pub fn effects_mut(&mut self, id: &String) -> Option<&mut [UIEffect]> {
        if let Some(track) = self.track_service.get(id) {
            Some(track.effects_mut())
        } else {
            None
        }
    }
    /// Set individual track volume
    pub fn set_volume(&mut self, id: String, volume: f32) {
        self.track_service.set_volume(id, volume, &mut self.tx);
    }
    pub fn set_mute(&mut self, id: String, mute: bool) {
        self.track_service.set_mute(id, mute, &mut self.tx);
    }
    pub fn toggle_solo(&mut self, id: String, modifier_pressed: bool) {
        self.track_service
            .toggle_solo(id, modifier_pressed, &mut self.tx);
    }
    pub fn select_track(&mut self, id: &String) {
        self.track_service.select(&id);
    }
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
    pub fn track_mut(&mut self, id: String) -> &mut MutableTrackCore {
        self.track_service.get_mut(id)
    }
    pub fn track_from_index(&self, index: usize) -> Option<TrackReferenceCore> {
        self.track_service.from_index(index)
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

    fn handle_pending_actions(&mut self) {
        for action in self.pending_actions.iter() {
            match action {
                ProjectStatePendingAction::DeleteTrack { id } => {
                    self.track_service.delete(&id, &mut self.tx);
                }
            }
        }
        self.pending_actions.clear();
    }
}
