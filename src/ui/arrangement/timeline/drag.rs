use crate::{
    core::{clip::AudioClip, state::ProjectState, track::DEFAULT_TRACK_HEIGHT},
    ui::{
        arrangement::clip::ClipView,
        arrangement::{
            row_layout::RowLayout,
            timeline::{Timeline, scroll::autoscroll},
        },
        theme::ThemeExt,
    },
};
use egui::{Rect, Stroke, Ui, pos2, vec2};
use tonique_engine::edit::ClipId;

#[derive(Clone)]
pub struct ClipDrag {
    pub clips: Vec<DraggedClip>,
    pub duplicate: bool,
    pub min_track_delta: i32,
}
#[derive(Clone)]
pub struct DraggedClip {
    pub clip: AudioClip,
    /// From the clip's start to the pointer, horizontally.
    pub grab_x: f32,
    pub track_index_delta: i32,
}

impl ClipDrag {
    pub fn dragged_ids(&self) -> Vec<ClipId> {
        self.clips.iter().map(|e| e.clip.id).collect()
    }
}

impl Timeline {
    pub fn handle_dragged_clips(
        &mut self,
        ui: &mut Ui,
        layout: &RowLayout,
        dragged_track_index: Option<usize>,
        viewport: Rect,
        dragged_clip: Option<AudioClip>,
        state: &mut ProjectState,
    ) {
        let mouse_pos = ui.ctx().input(|i| i.pointer.hover_pos());
        // Create dragging objects
        if self.clip_drag.is_none()
            && let Some(clip) = dragged_clip
            && let Some(mouse_pos) = mouse_pos
            && let Some(old_track) = dragged_track_index
        {
            // Grabbing a clip outside the selection (or outside its zone)
            // drags that clip alone.
            let grabbed = state.grid.x_to_beats(mouse_pos.x, viewport);
            let outside_zone = state
                .selection_bounds()
                .is_some_and(|b| grabbed < b.start_pos || grabbed > b.end_pos);
            if !state.is_clip_selected(clip.id) || outside_zone {
                state.select_clips(vec![clip.id]);
            }
            // With a zone, only its part of the clips moves.
            let zone = state.selection_bounds();
            let bpm = state.bpm();
            let mut clips = Vec::new();
            let mut new_selected_clips = Vec::new();
            let duplicate = ui.input(|i| i.modifiers.ctrl);
            let mut min_track_delta = 0;
            let tracks: Vec<_> = state.tracks().collect();
            for track in tracks {
                for clip in track.clips.iter() {
                    if state.is_clip_selected(clip.id) {
                        // Create a clone
                        let mut new_clip = if duplicate {
                            clip.with_id(state.new_clip_id())
                        } else {
                            clip.clone()
                        };
                        if let Some(b) = zone {
                            new_clip.crop(b.start_pos, b.end_pos, bpm);
                        }
                        // Update selected clips
                        new_selected_clips.push(new_clip.clone().id);
                        let track_index_delta = track.first_track_index as i32 - old_track as i32;
                        min_track_delta = min_track_delta.min(track_index_delta);
                        let x = state.grid.beats_to_x(new_clip.position, viewport);
                        clips.push(DraggedClip {
                            clip: new_clip,
                            grab_x: mouse_pos.x - x,
                            track_index_delta,
                        });
                    }
                }
            }
            // Copies get selected. Moved clips already are, and keep their
            // zone until dropped, to split the clips at it.
            if duplicate {
                state.select_clips(new_selected_clips);
            }
            self.clip_drag = Some(ClipDrag {
                clips,
                duplicate,
                min_track_delta,
            });
        }

        // Render clips while dragging
        if let Some(mut clip_drag) = self.clip_drag.take()
            && let Some(mouse_pos) = mouse_pos
        {
            // Find track at mouse position
            // Index of the track at mouse position
            let mouse_track_index = layout
                .track_at(mouse_pos.y)
                .map_or(state.track_count(), |(index, _)| index)
                .max(-clip_drag.min_track_delta as usize)
                as i32;

            // Snap the nearest clip edge: starts to the grid or to snap
            // targets (other clips, loop, edit cursor), ends to targets only.
            // Alt disables snapping.
            let mut beat_delta: f32 = f32::INFINITY;
            let mut snapped_to = None;
            if ui.input(|i| !i.modifiers.alt) {
                let targets = state.snap_targets(&clip_drag.dragged_ids());
                let bpm = state.bpm();
                for dragged in clip_drag.clips.iter() {
                    let start = state
                        .grid
                        .x_to_beats(mouse_pos.x - dragged.grab_x, viewport);
                    let end = start + dragged.clip.end(bpm) - dragged.clip.position;
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
            let first = clip_drag
                .clips
                .iter()
                .map(|e| state.grid.x_to_beats(mouse_pos.x - e.grab_x, viewport))
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
            for dragged in clip_drag.clips.iter_mut() {
                if let Some(duration) = dragged.clip.duration() {
                    // Calculate track index
                    let track_index =
                        (mouse_track_index + dragged.track_index_delta).max(0) as usize;

                    track_indexes.push(track_index);
                    // Calculate y pos
                    let y = layout.track_top(track_index);
                    // Calculate width
                    let width = state.grid.duration_to_width(duration, state.bpm());
                    // Calculate x pos
                    let new_position = state
                        .grid
                        .x_to_beats(mouse_pos.x - dragged.grab_x, viewport)
                        + beat_delta;
                    dragged.clip.position = new_position;
                    let x = state.grid.beats_to_x(new_position, viewport);

                    let mut show_waveform = true;
                    let mut color = ui.app_theme().text_muted;
                    let mut height = DEFAULT_TRACK_HEIGHT;
                    if let Some(t) = state.track_from_index(track_index) {
                        show_waveform = !t.collapsed;
                        color = t.color;
                        height = t.height;
                    }
                    let pos = pos2(x, y);
                    let size = vec2(width, height);
                    // Render Clip
                    ClipView {
                        clip: &dragged.clip,
                        rect: Rect::from_min_size(pos, size),
                        viewport,
                        color,
                        selected: true,
                        show_waveform,
                    }
                    .ui(ui, state);
                }
            }

            // Update state on mouse released
            if !ui.input(|i| i.pointer.primary_down()) {
                self.commit_drag(state, clip_drag, track_indexes);
            } else if !clip_drag.duplicate || ui.input(|i| i.modifiers.ctrl) {
                self.clip_drag = Some(clip_drag);
                autoscroll(ui, state, layout, viewport, mouse_pos);
            }
        }
    }

    fn commit_drag(
        &mut self,
        state: &mut ProjectState,
        clip_drag: ClipDrag,
        track_indexes: Vec<usize>,
    ) {
        let ids = clip_drag.dragged_ids();

        let mut tracks: Vec<_> = state.tracks().map(|t| t.id).collect();
        state.begin_batch();
        if !clip_drag.duplicate {
            // Only the zone's part moves: it keeps the clips' ids.
            state.split_at_zone();
        }
        for (i, dragged) in clip_drag.clips.iter().enumerate() {
            let track_index = track_indexes[i];
            // Create missing tracks
            while tracks.len() <= track_index {
                tracks.push(state.add_track());
            }

            let clone = dragged.clip.clone();

            let track_id = tracks[track_index];
            if clip_drag.duplicate {
                state.add_clips(&track_id, vec![clone]);
            } else {
                state.move_clip(&clone.id, &track_id, clone.position, &ids);
            }
        }
        state.commit_batch();
        // The zone stayed behind: keep the moved clips selected without it.
        state.select_clips(ids);
        self.clip_drag = None;
    }
}
