use egui::{CursorIcon, Painter, Rect, Sense, Stroke, Ui, vec2};
use tonique_engine::time::BeatPos;

use crate::{core::state::ProjectState, ui::arrangement::grid::PaintGrid, ui::theme::ThemeExt};

/// The loop lane, then the bars, then the time: a row each.
pub const RULER_HEIGHT: f32 = 3. * ROW_HEIGHT;
const ROW_HEIGHT: f32 = 12.;
/// Height of the loop lane at the top of the ruler; the rest seeks.
const LOOP_LANE_HEIGHT: f32 = ROW_HEIGHT;
/// How close to an edge of the loop brace grabs that edge, in points.
const EDGE_GRAB: f32 = LOOP_GRIP_WIDTH;
/// Space above and below the loop bar, in its lane.
const LOOP_INSET: f32 = 1.5;
/// The loop bar's end caps.
const LOOP_GRIP_WIDTH: f32 = 6.;

/// What a drag in the loop lane is doing.
#[derive(Clone, Copy)]
enum LoopDrag {
    Start,
    End,
    /// Moving the whole region, grabbed `offset` beats after its start.
    Move {
        offset: f64,
    },
    /// Drawing a new region from `anchor`.
    Create {
        anchor: BeatPos,
    },
}

pub struct Ruler {
    loop_drag: Option<LoopDrag>,
}

impl Ruler {
    pub fn new() -> Self {
        Self { loop_drag: None }
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &mut ProjectState) {
        ui.spacing_mut().interact_size.y = 0.;
        ui.horizontal(|ui| {
            // ColorBar for zoom control
            let ruler_rect = egui::Rect::from_min_size(
                egui::pos2(ui.min_rect().left(), ui.min_rect().top()),
                egui::vec2(ui.available_width(), RULER_HEIGHT),
            );
            let (nav_bar_response, painter) = ui.allocate_painter(
                vec2(ui.available_width(), RULER_HEIGHT),
                Sense::click_and_drag(),
            );

            // Draw rectangle
            painter.rect_filled(ruler_rect, 0.0, ui.app_theme().bg_raised);
            // Draw Labels, under the loop lane
            let labels = ruler_rect.with_min_y(ruler_rect.top() + LOOP_LANE_HEIGHT);
            state
                .grid
                .paint_labels(&painter, labels, state.bpm(), &ui.app_theme());
            let loop_lane =
                Rect::from_min_size(ruler_rect.min, vec2(ruler_rect.width(), LOOP_LANE_HEIGHT));
            let clicked_lane = self.loop_lane(ui, &painter, state, loop_lane);
            // Seek on click, scrub on drag
            if (nav_bar_response.clicked() || nav_bar_response.dragged() || clicked_lane)
                && let Some(mouse_pos) = ui.input(|i| i.pointer.interact_pos())
            {
                state.seek(state.grid.x_to_beats(mouse_pos.x, ruler_rect));
            }
            // Zoom (the loop lane covers part of the bar, so don't rely on
            // the bar's own hover)
            if ui.input(|i| i.smooth_scroll_delta.y != 0.0)
                && let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos())
                && ruler_rect.contains(mouse_pos)
            {
                let delta = ui.input(|i| i.smooth_scroll_delta.y);
                state.grid.zoom_around(delta, mouse_pos.x, ruler_rect);
            }
        });
    }

    /// The loop brace: drag its edges to resize it, its body to move it, or
    /// the empty lane to draw a new one; click it to toggle looping.
    /// Returns whether the empty lane was clicked (which seeks).
    fn loop_lane(
        &mut self,
        ui: &mut Ui,
        painter: &Painter,
        state: &mut ProjectState,
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
                    offset: ((x - x0) / pixels_per_beat) as f64,
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
                    let new_start = state.grid.snap_to_step(beats - offset).max(BeatPos::ZERO);
                    state.set_loop_range(new_start, new_start + (end - start));
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
        let x_range = state.grid.beats_to_x(start, lane)..=state.grid.beats_to_x(end, lane);
        self.paint_loop(
            ui,
            painter,
            Rect::from_x_y_ranges(x_range, lane.shrink2(vec2(0., LOOP_INSET)).y_range()),
            state.looping(),
            self.loop_drag.or(hovered),
        );
        clicked_lane
    }

    /// The loop region as a rounded bar with grips at its ends: the accent
    /// while looping, dim otherwise. The part under the pointer or being
    /// dragged (an edge, or the whole bar) lights up.
    fn paint_loop(
        &self,
        ui: &Ui,
        painter: &Painter,
        bar: Rect,
        looping: bool,
        active: Option<LoopDrag>,
    ) {
        let theme = ui.app_theme();
        let (color, on_color) = if looping {
            (theme.accent, theme.text_on_accent)
        } else {
            (theme.text_disabled, theme.bg_deep)
        };
        let whole = matches!(active, Some(LoopDrag::Move { .. }));
        let body = if whole { 0.85 } else { 0.6 };
        let radius = bar.height() / 2.;
        painter.rect_filled(bar, radius, color.gamma_multiply(body));

        // The edges: a solid cap, wider while grabbed, with a grip.
        let edges = [
            (bar.left(), 1., matches!(active, Some(LoopDrag::Start))),
            (bar.right(), -1., matches!(active, Some(LoopDrag::End))),
        ];
        // Too short for grips: just the bar.
        if bar.width() < 4. * LOOP_GRIP_WIDTH {
            return;
        }
        for (x, inward, grabbed) in edges {
            let width = if grabbed {
                LOOP_GRIP_WIDTH + 2.
            } else {
                LOOP_GRIP_WIDTH
            };
            let cap = Rect::from_x_y_ranges(
                (x.min(x + inward * width))..=(x.max(x + inward * width)),
                bar.y_range(),
            );
            let corners = if inward > 0. {
                egui::CornerRadius {
                    nw: radius as u8,
                    sw: radius as u8,
                    ..Default::default()
                }
            } else {
                egui::CornerRadius {
                    ne: radius as u8,
                    se: radius as u8,
                    ..Default::default()
                }
            };
            painter.rect_filled(cap, corners, color);
            let grip = cap.shrink2(vec2(0., 2.5)).y_range();
            for dx in [-1., 1.] {
                painter.vline(cap.center().x + dx, grip, Stroke::new(1., on_color));
            }
        }
    }
}
