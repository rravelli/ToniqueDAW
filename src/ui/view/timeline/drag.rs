use crate::{
    core::{
        clip::ClipCore,
        state::ToniqueProjectState,
        track::{DEFAULT_TRACK_HEIGHT, TrackCore, TrackSoloState},
    },
    ui::{
        clip::UIClip,
        track::HANDLE_HEIGHT,
        utils::{find_track_at, get_track_y},
        view::timeline::UITimeline,
    },
};
use egui::{Color32, Pos2, Rect, Ui, pos2, vec2};
use std::f32::INFINITY;

#[derive(Clone)]
pub struct DragState {
    pub elements: Vec<ClipDragState>,
    pub duplicate: bool,
    pub min_track_delta: i32,
}
#[derive(Clone)]
pub struct ClipDragState {
    pub clip: ClipCore,
    pub mouse_delta: Pos2,
    pub track_index_delta: i32,
}

impl DragState {
    pub fn dragged_ids(&self) -> Vec<String> {
        self.elements.iter().map(|e| e.clip.id.clone()).collect()
    }
}

impl UITimeline {
    pub fn handle_dragged_clips(
        &mut self,
        ui: &mut Ui,
        dragged_track_index: Option<usize>,
        viewport: Rect,
        dragged_clip: Option<ClipCore>,
        state: &mut ToniqueProjectState,
    ) {
        let mouse_pos = ui.ctx().input(|i| i.pointer.hover_pos());
        // Create dragging objects
        if self.drag_state.is_none()
            && let Some(clip) = dragged_clip
            && let Some(mouse_pos) = mouse_pos
            && let Some(old_track) = dragged_track_index
        {
            self.selected_clips.bounds = None;

            if !self.selected_clips.clip_ids.contains(&clip.id) {
                self.selected_clips.clip_ids = vec![clip.id];
            }
            let mut elements = Vec::new();
            let mut new_selected_clips = Vec::new();
            let duplicate = ui.input(|i| i.modifiers.ctrl);
            let mut y = viewport.top();
            let mut min_track_delta = 0;
            let tracks: Vec<_> = state.tracks().collect();
            for track in tracks {
                for clip in track.clips.iter() {
                    if self.selected_clips.clip_ids.contains(&clip.id) {
                        // Create a clone
                        let new_clip = if duplicate {
                            clip.clone_with_new_id()
                        } else {
                            clip.clone()
                        };
                        // Update selected clips
                        new_selected_clips.push(new_clip.clone().id);
                        let track_index_delta = track.index as i32 - old_track as i32;
                        min_track_delta = min_track_delta.min(track_index_delta);
                        let x = state.grid.beats_to_x(clip.position, viewport);
                        elements.push(ClipDragState {
                            clip: new_clip,
                            mouse_delta: pos2(mouse_pos.x - x, mouse_pos.y - y),
                            track_index_delta,
                        });
                    }
                }
                y += track.height + HANDLE_HEIGHT;
            }
            self.selected_clips.clip_ids = new_selected_clips;
            self.drag_state = Some(DragState {
                elements,
                duplicate,
                min_track_delta,
            });
        }

        // Render clips while dragging
        if let Some(mut drag_state) = self.drag_state.take()
            && let Some(mouse_pos) = mouse_pos
        {
            // Find track at mouse position
            let (track, _) = find_track_at(state, viewport, mouse_pos.y);

            // Index of the track at mouse position
            let mouse_track_index = track
                .map_or(state.track_len(), |t| t.index)
                .max(-drag_state.min_track_delta as usize)
                as i32;

            // Find nearest grid to snap to
            let mut beat_delta: f32 = INFINITY;
            if ui.input(|i| !i.modifiers.alt) {
                for element in drag_state.elements.iter() {
                    let new_position = state
                        .grid
                        .x_to_beats(mouse_pos.x - element.mouse_delta.x, viewport);
                    let snapped_position = state.grid.snap_at_grid_option(new_position);
                    if let Some(pos) = snapped_position
                        && (pos - new_position).abs() < beat_delta.abs()
                    {
                        beat_delta = pos - new_position;
                    }
                }
            }
            // No clip are snapped
            if beat_delta == INFINITY {
                beat_delta = 0.;
            }

            let mut track_indexes = Vec::new();
            for element in drag_state.elements.iter_mut() {
                if let Some(duration) = element.clip.duration() {
                    // Calculate track index
                    let track_index =
                        (mouse_track_index + element.track_index_delta).max(0) as usize;

                    track_indexes.push(track_index);
                    // Calculate y pos
                    let y = get_track_y(track_index, viewport, state);
                    // Calculate width
                    let width = state.grid.duration_to_width(duration, state.bpm());
                    // Calculate x pos
                    let new_position = state
                        .grid
                        .x_to_beats(mouse_pos.x - element.mouse_delta.x, viewport)
                        + beat_delta;
                    element.clip.position = new_position;
                    let x = state.grid.beats_to_x(new_position, viewport);

                    let mut show_waveform = true;
                    let mut color = Color32::WHITE;
                    let mut height = DEFAULT_TRACK_HEIGHT;
                    if let Some(t) = state.track_from_index(track_index) {
                        show_waveform = !t.closed;
                        color = t.color;
                        height = t.height;
                    }
                    let pos = pos2(x, y - state.grid.offset.y);
                    let size = vec2(width, height);
                    // Render Clip
                    UIClip::new().ui(
                        ui,
                        pos,
                        size,
                        viewport,
                        true,
                        &element.clip,
                        state,
                        show_waveform,
                        color,
                    );
                }
            }

            // Update state on mouse released
            if !ui.input(|i| i.pointer.primary_down()) {
                self.commit_drag(state, drag_state, track_indexes);
            } else if !drag_state.duplicate || ui.input(|i| i.modifiers.ctrl) {
                self.drag_state = Some(drag_state);
            }
        }
    }

    fn commit_drag(
        &mut self,
        state: &mut ToniqueProjectState,
        drag_state: DragState,
        track_indexes: Vec<usize>,
    ) {
        let ids = drag_state.dragged_ids();

        let mut tracks: Vec<_> = state.tracks().collect();
        state.begin_batch();
        for (i, element) in drag_state.elements.iter().enumerate() {
            let length = tracks.len();
            let track_index = track_indexes[i];
            // Create
            if track_index >= length {
                for _ in 0..(track_index - length + 1) {
                    let new_track = TrackCore::new();
                    tracks.push(new_track.get_reference(0, false, TrackSoloState::NotSoloing));
                    state.add_track(new_track);
                }
            }

            let clone = element.clip.clone();

            let track = &tracks[track_index];
            let track_id = track.id.clone();
            if drag_state.duplicate {
                state.add_clips(&track_id, vec![clone]);
            } else {
                state.move_clip(&clone.id, &track_id, clone.position, &ids);
            }
        }
        state.commit_batch();
        self.drag_state = None;
    }
}
