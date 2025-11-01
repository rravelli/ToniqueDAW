use egui::{Sense, Ui, Vec2, vec2};
use egui_phosphor::fill::{ARROWS_IN_LINE_VERTICAL, ARROWS_OUT_LINE_VERTICAL, LINE_SEGMENTS};

use crate::{
    core::state::ToniqueProjectState,
    ui::{view::tracks::DRAGGER_WIDTH, widget::square_button::SquareButton},
};

pub struct UINavigationBar {}

pub const NAVIGATION_BAR_HEIGHT: f32 = 30.;

impl UINavigationBar {
    pub fn new() -> Self {
        Self {}
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState, track_width: f32) {
        ui.spacing_mut().interact_size.y = 0.;
        ui.horizontal(|ui| {
            // Rectangle for zoom control
            let nav_bar_rect = egui::Rect::from_min_size(
                egui::pos2(ui.min_rect().left(), ui.min_rect().top()),
                egui::vec2(ui.available_width() - track_width, NAVIGATION_BAR_HEIGHT),
            );
            let (nav_bar_response, painter) = ui.allocate_painter(
                vec2(ui.available_width() - track_width, NAVIGATION_BAR_HEIGHT),
                Sense::click_and_drag(),
            );

            // Draw rectangle
            painter.rect_filled(nav_bar_rect, 0.0, egui::Color32::from_gray(80));
            // Draw Labels
            state
                .grid
                .render_labels(&painter, nav_bar_rect, state.bpm());
            // Move to cursor on click
            if nav_bar_response.clicked()
                && let Some(mouse_pos) = nav_bar_response.interact_pointer_pos()
            {
                state.set_playback_position(state.grid.x_to_beats(mouse_pos.x, nav_bar_rect));
            }
            // Zoom
            if ui.input(|i| i.raw_scroll_delta.y != 0.0)
                && let Some(mouse_pos) = nav_bar_response.hover_pos()
            {
                let delta = ui.input(|i| i.smooth_scroll_delta.y);
                state.grid.zoom_around(delta, mouse_pos.x, nav_bar_rect);
            }
            ui.add_space(DRAGGER_WIDTH + 4.0);
            self.right_ui(ui, state);
        });
    }

    fn right_ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
            if ui
                .add(SquareButton::new(ARROWS_OUT_LINE_VERTICAL).tooltip("Open All"))
                .clicked()
            {
                state.set_all_close(false);
            };
            if ui
                .add(SquareButton::new(ARROWS_IN_LINE_VERTICAL).tooltip("Close All"))
                .clicked()
            {
                state.set_all_close(true);
            };
            ui.add_enabled(false, SquareButton::new(LINE_SEGMENTS).tooltip("Automate"));
        });
    }
}
