use egui::{Color32, Pos2, Rect, Response, Stroke, StrokeKind, Ui};

use crate::{
    core::state::ToniqueProjectState,
    ui::{track::HANDLE_HEIGHT, track_manager::TrackManager, view::timeline::UITimeline},
};

#[derive(Debug, Clone, Copy)]
pub struct SelectionBounds {
    pub start_track_index: usize,
    pub start_pos: f32,
    pub end_track_index: usize,
    pub end_pos: f32,
}

#[derive(Debug, Clone)]
pub struct ClipSelection {
    pub clip_ids: Vec<String>,
    pub bounds: Option<SelectionBounds>,
}

pub struct Multiselect {
    start_pos: f32,
    start_track_index: usize,
}

impl ClipSelection {
    pub fn reset(&mut self) {
        self.bounds = None;
        self.clip_ids.clear();
    }

    pub fn new() -> Self {
        Self {
            bounds: None,
            clip_ids: vec![],
        }
    }
}

impl UITimeline {
    pub fn handle_multiselect(
        &mut self,
        ui: &mut Ui,
        state: &mut ToniqueProjectState,
        response: &Response,
    ) {
        if ui.input(|i| i.pointer.primary_down()) {
            self.clicked_pos = response.interact_pointer_pos();
        }

        if response.drag_started()
            && let Some(mouse_pos) = self.clicked_pos
        {
            let (track, _) = self.find_track_at(state, response.rect, mouse_pos.y);

            let beat_pos = state.grid.x_to_beats(mouse_pos.x, response.rect);
            let snapped = state
                .grid
                .snap_at_grid_with_threshold(beat_pos, 1.)
                .unwrap_or(beat_pos);

            if state.track_len() > 0 {
                let index = track.map_or(state.track_len() - 1, |t| t.index);
                self.multiselect_start = Some(Multiselect {
                    start_pos: snapped,
                    start_track_index: index,
                });
            }
        }
        if !response.dragged() {
            self.multiselect_start = None;
        }

        if let Some(start) = &self.multiselect_start
            && let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos())
        {
            let (current_track, _) = self.find_track_at(state, response.rect, mouse_pos.y);

            let position = state.grid.x_to_beats(mouse_pos.x, response.rect);
            let current_pos = state
                .grid
                .snap_at_grid_with_threshold(position, 1.0)
                .unwrap_or(position);
            let length = state.track_len();
            let track_index = current_track.map_or(length - 1, |t| t.index);
            let min_index = track_index.min(start.start_track_index);
            let max_index = track_index.max(start.start_track_index);

            let min_pos = current_pos.min(start.start_pos);
            let max_pos = current_pos.max(start.start_pos);

            let mut min_point = Pos2::ZERO;
            let mut max_point = Pos2::ZERO;
            self.selected_clips.reset();
            self.selected_clips.bounds = Some(SelectionBounds {
                start_track_index: min_index,
                start_pos: min_pos,
                end_track_index: max_index,
                end_pos: max_pos,
            });
            let mut y = response.rect.top();
            for (track_index, track) in state.tracks().enumerate() {
                if track_index == min_index {
                    min_point = Pos2::new(state.grid.beats_to_x(min_pos, response.rect), y);
                }
                if min_index <= track_index && track_index <= max_index {
                    for clip in track.clips.iter() {
                        let end = clip.end(state.bpm());
                        if end >= min_pos && clip.position < max_pos {
                            self.selected_clips.clip_ids.push(clip.id.clone());
                        }
                    }
                }
                y += track.height;
                if track_index == max_index {
                    max_point = Pos2::new(state.grid.beats_to_x(max_pos, response.rect), y);
                }
                y += HANDLE_HEIGHT;
            }
            let select_rect = Rect::from_min_max(min_point, max_point);

            ui.painter().rect_stroke(
                select_rect,
                0.,
                Stroke::new(1. / ui.pixels_per_point(), Color32::from_white_alpha(250)),
                StrokeKind::Inside,
            );
        }

        if let Some(bounds) = self.selected_clips.bounds {
            let min_point = Pos2::new(
                state.grid.beats_to_x(bounds.start_pos, response.rect),
                TrackManager::new().get_track_y(bounds.start_track_index, response.rect, state),
            );

            let height = state
                .track_from_index(bounds.end_track_index)
                .map_or(0., |t| t.height);

            let max_point = Pos2::new(
                state.grid.beats_to_x(bounds.end_pos, response.rect),
                TrackManager::new().get_track_y(bounds.end_track_index, response.rect, state)
                    + height,
            );
            let rect = Rect::from_min_max(min_point, max_point);
            let painter = ui.painter_at(rect);
            painter.rect(
                rect,
                2.0,
                Color32::LIGHT_BLUE.gamma_multiply(0.1),
                Stroke::new(1. / ui.pixels_per_point(), Color32::WHITE),
                StrokeKind::Inside,
            );
        }
    }
}
