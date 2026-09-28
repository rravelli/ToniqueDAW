use crate::{
    core::state::{ProjectState, SelectionBounds},
    ui::{
        arrangement::{row_layout::RowLayout, timeline::Timeline},
        theme::ThemeExt,
    },
};
use egui::{Pos2, Rect, Response, Shape, Stroke, Ui};

/// A rubber-band selection being drawn, from where it started.
pub struct RubberBand {
    start_pos: f32,
    start_track_index: usize,
}

impl Timeline {
    pub fn rubber_band_ui(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        layout: &RowLayout,
        response: &Response,
    ) {
        if ui.input(|i| i.pointer.primary_down()) {
            self.press_pos = response.interact_pointer_pos();
        }

        if response.drag_started()
            && let Some(mouse_pos) = self.press_pos
        {
            let track = layout.track_at(mouse_pos.y).map(|(index, _)| index);

            let beat_pos = state.grid.x_to_beats(mouse_pos.x, response.rect);
            let snapped = state
                .grid
                .snap_at_grid_with_threshold(beat_pos, 1.)
                .unwrap_or(beat_pos);

            if state.track_count() > 0 {
                let index = track.unwrap_or(state.track_count() - 1);
                self.rubber_band = Some(RubberBand {
                    start_pos: snapped,
                    start_track_index: index,
                });
            }
        }
        if !response.dragged() {
            self.rubber_band = None;
        }

        if let Some(start) = &self.rubber_band
            && let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos())
        {
            let current_track = layout.track_at(mouse_pos.y).map(|(index, _)| index);

            let position = state.grid.x_to_beats(mouse_pos.x, response.rect);
            let current_pos = state
                .grid
                .snap_at_grid_with_threshold(position, 1.0)
                .unwrap_or(position);
            let length = state.track_count();
            let track_index = current_track.unwrap_or(length - 1);
            state.select_in_bounds(SelectionBounds::between(
                (start.start_track_index, start.start_pos),
                (track_index, current_pos),
            ));
        }

        if let Some(bounds) = state.selection_bounds() {
            self.paint_selection_zone(ui, state, layout, bounds, response.rect);
        }
    }

    fn paint_selection_zone(
        &self,
        ui: &Ui,
        state: &ProjectState,
        layout: &RowLayout,
        bounds: SelectionBounds,
        viewport: Rect,
    ) {
        let min_point = Pos2::new(
            state.grid.beats_to_x(bounds.start_pos, viewport),
            layout.track_top(bounds.start_track_index),
        );
        // Down to the bottom of the row showing the last track (its
        // collapsed group's, if hidden).
        let bottom = layout
            .track_row(bounds.end_track_index)
            .map_or(layout.track_top(bounds.end_track_index), |r| r.y.max);
        let max_point = Pos2::new(state.grid.beats_to_x(bounds.end_pos, viewport), bottom);
        let zone = Rect::from_min_max(min_point, max_point);
        let theme = ui.app_theme();
        let painter = ui.painter();
        painter.rect_filled(zone, 2.0, theme.selection_fill);
        // Dashed: reads as an area being drawn, not as something selected.
        let edge = zone.shrink(0.5);
        let outline = [
            edge.left_top(),
            edge.right_top(),
            edge.right_bottom(),
            edge.left_bottom(),
            edge.left_top(),
        ];
        painter.extend(Shape::dashed_line(
            &outline,
            Stroke::new(1. / ui.pixels_per_point(), theme.selection_stroke),
            4.,
            3.,
        ));
    }
}
