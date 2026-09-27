use egui::{CursorIcon, Painter, Rect, Sense, Stroke, Ui, Vec2, vec2};
use egui_phosphor::fill::{ARROWS_IN_LINE_VERTICAL, ARROWS_OUT_LINE_VERTICAL, LINE_SEGMENTS};

use crate::{
    core::state::ToniqueProjectState,
    ui::{theme::ThemeExt, view::tracks::DRAGGER_WIDTH, widget::square_button::SquareButton},
};

pub const NAVIGATION_BAR_HEIGHT: f32 = 30.;
/// Height of the loop lane at the top of the ruler; the rest seeks.
const LOOP_LANE_HEIGHT: f32 = 14.;
/// How close to an edge of the loop brace grabs that edge, in points.
const EDGE_GRAB: f32 = 5.;

/// What a drag in the loop lane is doing.
#[derive(Clone, Copy)]
enum LoopDrag {
    Start,
    End,
    /// Moving the whole region, grabbed `offset` beats after its start.
    Move {
        offset: f32,
    },
    /// Drawing a new region from `anchor`.
    Create {
        anchor: f32,
    },
}

pub struct UINavigationBar {
    loop_drag: Option<LoopDrag>,
}

impl UINavigationBar {
    pub fn new() -> Self {
        Self { loop_drag: None }
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
            painter.rect_filled(nav_bar_rect, 0.0, ui.app_theme().bg_raised);
            // Draw Labels
            state
                .grid
                .render_labels(&painter, nav_bar_rect, state.bpm(), &ui.app_theme());
            let loop_lane = Rect::from_min_size(
                nav_bar_rect.min,
                vec2(nav_bar_rect.width(), LOOP_LANE_HEIGHT),
            );
            let clicked_lane = self.loop_lane(ui, &painter, state, loop_lane);
            // Seek on click, scrub on drag
            if (nav_bar_response.clicked() || nav_bar_response.dragged() || clicked_lane)
                && let Some(mouse_pos) = ui.input(|i| i.pointer.interact_pos())
            {
                state.seek(state.grid.x_to_beats(mouse_pos.x, nav_bar_rect));
            }
            // Zoom (the loop lane covers part of the bar, so don't rely on
            // the bar's own hover)
            if ui.input(|i| i.smooth_scroll_delta.y != 0.0)
                && let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos())
                && nav_bar_rect.contains(mouse_pos)
            {
                let delta = ui.input(|i| i.smooth_scroll_delta.y);
                state.grid.zoom_around(delta, mouse_pos.x, nav_bar_rect);
            }
            ui.add_space(DRAGGER_WIDTH + 4.0);
            self.right_ui(ui, state);
        });
    }

    /// The loop brace: drag its edges to resize it, its body to move it, or
    /// the empty lane to draw a new one; click it to toggle looping.
    /// Returns whether the empty lane was clicked (which seeks).
    fn loop_lane(
        &mut self,
        ui: &mut Ui,
        painter: &Painter,
        state: &mut ToniqueProjectState,
        lane: Rect,
    ) -> bool {
        let response = ui.interact(lane, ui.id().with("loop_lane"), Sense::click_and_drag());
        let (start, end) = state.loop_range();
        let (x0, x1) = (
            state.grid.beats_to_x(start, lane),
            state.grid.beats_to_x(end, lane),
        );
        let pixels_per_beat = state.grid.pixels_per_beat();
        let grab_at = |x: f32| -> Option<LoopDrag> {
            let (d0, d1) = ((x - x0).abs(), (x - x1).abs());
            if d0.min(d1) <= EDGE_GRAB {
                Some(if d0 <= d1 {
                    LoopDrag::Start
                } else {
                    LoopDrag::End
                })
            } else if x0 < x && x < x1 {
                Some(LoopDrag::Move {
                    offset: (x - x0) / pixels_per_beat,
                })
            } else {
                None
            }
        };

        if response.drag_started()
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            let anchor = state
                .grid
                .snap_to_step(state.grid.x_to_beats(origin.x, lane));
            self.loop_drag = Some(grab_at(origin.x).unwrap_or(LoopDrag::Create { anchor }));
        }
        if response.dragged()
            && let Some(drag) = self.loop_drag
            && let Some(pos) = response.interact_pointer_pos()
        {
            let beats = state.grid.x_to_beats(pos.x, lane);
            let snapped = state.grid.snap_to_step(beats);
            let step = state.grid.step_beats();
            match drag {
                LoopDrag::Start => state.set_loop_range(snapped.min(end - step), end),
                LoopDrag::End => state.set_loop_range(start, snapped.max(start + step)),
                LoopDrag::Move { offset } => {
                    let new_start = state.grid.snap_to_step(beats - offset).max(0.);
                    state.set_loop_range(new_start, new_start + end - start);
                }
                LoopDrag::Create { anchor } => {
                    if snapped != anchor {
                        state.set_loop_range(anchor, snapped);
                        if !state.looping() {
                            state.set_looping(true);
                        }
                    }
                }
            }
        }
        if response.drag_stopped() {
            self.loop_drag = None;
        }

        let hovered = response.hover_pos().and_then(|p| grab_at(p.x));
        let mut clicked_lane = false;
        if response.clicked() {
            match hovered {
                Some(_) => state.set_looping(!state.looping()),
                None => clicked_lane = true,
            }
        }
        match self.loop_drag.or(hovered) {
            Some(LoopDrag::Start | LoopDrag::End) => {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal)
            }
            Some(LoopDrag::Move { .. }) if self.loop_drag.is_some() => {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing)
            }
            Some(LoopDrag::Move { .. }) => ui.ctx().set_cursor_icon(CursorIcon::Grab),
            _ => {}
        }

        // Paint with the range as it is after this frame's edits.
        let (start, end) = state.loop_range();
        let brace = Rect::from_x_y_ranges(
            state.grid.beats_to_x(start, lane)..=state.grid.beats_to_x(end, lane),
            lane.y_range(),
        );
        let theme = ui.app_theme();
        let color = if state.looping() {
            theme.accent
        } else {
            theme.text_disabled
        };
        let fill = if response.hovered() || self.loop_drag.is_some() {
            0.45
        } else {
            0.3
        };
        painter.rect_filled(brace, 0., color.gamma_multiply(fill));
        painter.rect_filled(
            Rect::from_x_y_ranges(brace.x_range(), lane.top()..=lane.top() + 3.),
            0.,
            color,
        );
        for x in [brace.left(), brace.right()] {
            painter.vline(x, lane.y_range(), Stroke::new(2., color));
        }
        clicked_lane
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
