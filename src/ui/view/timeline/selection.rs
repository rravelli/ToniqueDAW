use crate::{
    core::state::{SelectionBounds, ToniqueProjectState},
    ui::{
        utils::{find_track_at, get_track_y},
        view::timeline::UITimeline,
    },
};
use egui::{Color32, Pos2, Rect, Response, Stroke, StrokeKind, Ui};

/// A rubber-band selection being drawn, from where it started.
pub struct Multiselect {
    start_pos: f32,
    start_track_index: usize,
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
            let (track, _) = find_track_at(state, response.rect, mouse_pos.y);

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
            let (current_track, _) = find_track_at(state, response.rect, mouse_pos.y);

            let position = state.grid.x_to_beats(mouse_pos.x, response.rect);
            let current_pos = state
                .grid
                .snap_at_grid_with_threshold(position, 1.0)
                .unwrap_or(position);
            let length = state.track_len();
            let track_index = current_track.map_or(length - 1, |t| t.index);
            state.select_in_bounds(SelectionBounds::between(
                (start.start_track_index, start.start_pos),
                (track_index, current_pos),
            ));
        }

        if let Some(bounds) = state.selection_bounds() {
            self.render_zone(ui, state, bounds, response.rect);
        }
    }

    fn render_zone(
        &mut self,
        ui: &mut Ui,
        state: &mut ToniqueProjectState,
        bounds: SelectionBounds,
        viewport: Rect,
    ) {
        let min_point = Pos2::new(
            state.grid.beats_to_x(bounds.start_pos, viewport),
            get_track_y(bounds.start_track_index, viewport, state) - state.grid.offset.y,
        );

        let height = state
            .track_from_index(bounds.end_track_index)
            .map_or(0., |t| t.height);

        let max_point = Pos2::new(
            state.grid.beats_to_x(bounds.end_pos, viewport),
            get_track_y(bounds.end_track_index, viewport, state) + height - state.grid.offset.y,
        );
        let zone = Rect::from_min_max(min_point, max_point);
        let painter = ui.painter();
        painter.rect(
            zone,
            2.0,
            Color32::LIGHT_BLUE.gamma_multiply(0.1),
            Stroke::new(1. / ui.pixels_per_point(), Color32::WHITE),
            StrokeKind::Inside,
        );
    }
}
