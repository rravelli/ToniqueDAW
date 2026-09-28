use crate::{
    core::{clip::ClipCore, state::ToniqueProjectState, track::DEFAULT_TRACK_HEIGHT},
    ui::{
        clip::ClipView,
        theme::ThemeExt,
        track::HANDLE_HEIGHT,
        utils::{find_track_at, track_y},
        view::timeline::{Timeline, scroll::autoscroll},
    },
};
use egui::{Pos2, Rect, Stroke, Ui, pos2, vec2};
use tonique_engine::edit::ClipId;

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
    pub fn dragged_ids(&self) -> Vec<ClipId> {
        self.elements.iter().map(|e| e.clip.id).collect()
    }
}

impl Timeline {
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
            if !state.is_clip_selected(clip.id) {
                state.select_clips(vec![clip.id]);
            }
            let mut elements = Vec::new();
            let mut new_selected_clips = Vec::new();
            let duplicate = ui.input(|i| i.modifiers.ctrl);
            let mut y = viewport.top();
            let mut min_track_delta = 0;
            let tracks: Vec<_> = state.tracks().collect();
            for track in tracks {
                for clip in track.clips.iter() {
                    if state.is_clip_selected(clip.id) {
                        // Create a clone
                        let new_clip = if duplicate {
                            clip.with_id(state.new_clip_id())
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
            state.select_clips(new_selected_clips);
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
                .map_or(state.track_count(), |t| t.index)
                .max(-drag_state.min_track_delta as usize)
                as i32;

            // Snap the nearest clip edge: starts to the grid or to snap
            // targets (other clips, loop, edit cursor), ends to targets only.
            // Alt disables snapping.
            let mut beat_delta: f32 = f32::INFINITY;
            let mut snapped_to = None;
            if ui.input(|i| !i.modifiers.alt) {
                let targets = state.snap_targets(&drag_state.dragged_ids());
                let bpm = state.bpm();
                for element in drag_state.elements.iter() {
                    let start = state
                        .grid
                        .x_to_beats(mouse_pos.x - element.mouse_delta.x, viewport);
                    let end = start + element.clip.end(bpm) - element.clip.position;
                    let candidates = [
                        state
                            .grid
                            .snap_to_targets(start, &targets)
                            .map(|s| (s, start)),
                        state
                            .grid
                            .snap_to_targets(end, &targets)
                            .filter(|(_, target)| *target)
                            .map(|s| (s, end)),
                    ];
                    for ((pos, target), edge) in candidates.into_iter().flatten() {
                        if (pos - edge).abs() < beat_delta.abs() {
                            beat_delta = pos - edge;
                            snapped_to = target.then_some(pos);
                        }
                    }
                }
            }
            // No clip are snapped
            if beat_delta == f32::INFINITY {
                beat_delta = 0.;
            }
            // Keep the group from starting before the first beat.
            let first = drag_state
                .elements
                .iter()
                .map(|e| {
                    state
                        .grid
                        .x_to_beats(mouse_pos.x - e.mouse_delta.x, viewport)
                })
                .fold(f32::INFINITY, f32::min);
            if -first > beat_delta {
                beat_delta = -first;
                snapped_to = None;
            }
            // Show what the clips snapped to.
            if let Some(beats) = snapped_to {
                let x = state.grid.beats_to_x(beats, viewport);
                ui.painter_at(viewport).vline(
                    x,
                    viewport.y_range(),
                    Stroke::new(1., ui.app_theme().text_muted),
                );
            }

            let mut track_indexes = Vec::new();
            for element in drag_state.elements.iter_mut() {
                if let Some(duration) = element.clip.duration() {
                    // Calculate track index
                    let track_index =
                        (mouse_track_index + element.track_index_delta).max(0) as usize;

                    track_indexes.push(track_index);
                    // Calculate y pos
                    let y = track_y(track_index, viewport, state);
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
                    let mut color = ui.app_theme().text_muted;
                    let mut height = DEFAULT_TRACK_HEIGHT;
                    if let Some(t) = state.track_from_index(track_index) {
                        show_waveform = !t.closed;
                        color = t.color;
                        height = t.height;
                    }
                    let pos = pos2(x, y - state.grid.offset.y);
                    let size = vec2(width, height);
                    // Render Clip
                    ClipView::new().ui(
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
                autoscroll(ui, state, viewport, mouse_pos);
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

        let mut tracks: Vec<_> = state.tracks().map(|t| t.id).collect();
        state.begin_batch();
        for (i, element) in drag_state.elements.iter().enumerate() {
            let track_index = track_indexes[i];
            // Create missing tracks
            while tracks.len() <= track_index {
                tracks.push(state.add_track());
            }

            let clone = element.clip.clone();

            let track_id = tracks[track_index];
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
