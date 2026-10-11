use crate::{
    core::state::{ProjectState, SelectionBounds},
    ui::{
        arrangement::{row_layout::RowLayout, timeline::Timeline},
        theme::ThemeExt,
    },
};
use egui::{Pos2, Rect, Response, Shape, Stroke, Ui};
use tonique_engine::time::BeatPos;

/// A rubber-band selection being drawn, from where it started.
pub struct RubberBand {
    start_pos: BeatPos,
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
        // Near grid lines it snaps to them, as trims do; Alt keeps it free.
        let free = ui.input(|i| i.modifiers.alt);
        let snap = |beats: BeatPos| {
            if free {
                beats
            } else {
                state.grid.snap_at_grid(beats)
            }
        };
        // From where the button went down: egui starts drags a few points
        // later.
        if response.drag_started()
            && let Some(mouse_pos) = ui.input(|i| i.pointer.press_origin())
        {
            let track = layout.track_at(mouse_pos.y).map(|(index, _)| index);
            let snapped = snap(state.grid.x_to_beats(mouse_pos.x, response.rect));

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

            let current_pos = snap(state.grid.x_to_beats(mouse_pos.x, response.rect));
            let length = state.track_count();
            let track_index = current_track.unwrap_or(length - 1);
            state.select_in_bounds(SelectionBounds::between(
                (start.start_track_index, start.start_pos),
                (track_index, current_pos),
            ));
        }

        // The veil, under the clips.
        if let Some(zone) = zone_rect(state, layout, response.rect) {
            let theme = ui.app_theme();
            ui.painter_at(response.rect)
                .rect_filled(zone, 2.0, theme.selection_fill);
        }
    }

    /// The zone's dashed outline: reads as an area being drawn, not as
    /// something selected. Over the clips.
    pub fn paint_zone_outline(
        &self,
        ui: &Ui,
        state: &ProjectState,
        layout: &RowLayout,
        viewport: Rect,
    ) {
        let Some(zone) = zone_rect(state, layout, viewport) else {
            return;
        };
        let theme = ui.app_theme();
        let edge = zone.shrink(0.5);
        let outline = [
            edge.left_top(),
            edge.right_top(),
            edge.right_bottom(),
            edge.left_bottom(),
            edge.left_top(),
        ];
        ui.painter_at(viewport).extend(Shape::dashed_line(
            &outline,
            Stroke::new(1., theme.selection_stroke),
            5.,
            3.,
        ));
    }
}

/// Where the selection zone is on screen, if there's one.
fn zone_rect(state: &ProjectState, layout: &RowLayout, viewport: Rect) -> Option<Rect> {
    let bounds: SelectionBounds = state.selection_bounds()?;
    let min_point = Pos2::new(
        state.grid.beats_to_x(bounds.start_pos, viewport),
        layout.track_top(bounds.start_track_index),
    );
    // Down to the bottom of the row showing the last track (its collapsed
    // group's, if hidden).
    let bottom = layout
        .track_row(bounds.end_track_index)
        .map_or(layout.track_top(bounds.end_track_index), |r| r.y.max);
    let max_point = Pos2::new(state.grid.beats_to_x(bounds.end_pos, viewport), bottom);
    Some(Rect::from_min_max(min_point, max_point))
}
