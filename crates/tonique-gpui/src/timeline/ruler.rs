//! What the ruler's loop lane holds, as egui's: the loop region, a rounded
//! bar with gripped caps at its ends (drag a cap to resize it, the bar to
//! move it, or the empty lane to draw a new one; click the bar to toggle
//! looping), and the playhead's head, dragged to scrub.

use crate::model::HoldsProject;
use crate::timeline::Arrangement;
use gpui::{App, Corners, PathBuilder, Window, point, px};
use tonique_engine::time::BeatPos;

use crate::{
    geometry::Rect,
    theme::{hsla, with_alpha},
    timeline::{LOOP_HEIGHT, RULER_HEIGHT, input::TimelineDrag, paint::Canvas, scene::Scene},
};

/// The playhead's head: its width, its height to its point, and how far
/// down its sides go before narrowing to it.
const HEAD_WIDTH: f32 = 8.;
const HEAD_HEIGHT: f32 = 10.;
const HEAD_SHOULDER: f32 = 0.7;
/// The playhead's line, and its opacity.
const PLAYHEAD_WIDTH: f32 = 2.;
const PLAYHEAD_ALPHA: u8 = 160;

/// The caps' width, and how close to an end grabs that end.
const GRIP_WIDTH: f32 = 6.;
/// Space above and below the bar, in its lane.
const INSET: f32 = 1.5;
/// The bar's opacity between its caps, and while moved.
const BODY_ALPHA: f32 = 0.6;
const MOVED_ALPHA: f32 = 0.85;

/// The part of the loop a press grabbed, or the pointer is over.
#[derive(Clone, Copy, PartialEq)]
pub enum LoopGrab {
    Start,
    End,
    /// The whole region, grabbed `offset` beats after its start.
    Move {
        offset: f64,
    },
    /// A new region drawn from `anchor`.
    Create {
        anchor: BeatPos,
    },
}

impl Arrangement {
    /// Whether the pointer at (`local_x`, `local_y`) grabs the playhead's
    /// head: around it, a little wider and taller than it.
    pub(super) fn on_playhead_head(&self, local_x: f32, local_y: f32, context: &App) -> bool {
        let playhead_x = self
            .state(context)
            .grid
            .beats_to_x(self.state(context).playhead(), self.viewport());
        let around = Rect::new(
            playhead_x - HEAD_WIDTH,
            0.,
            2. * HEAD_WIDTH,
            HEAD_HEIGHT + 6.,
        );
        around.contains(local_x, local_y)
    }

    /// The playhead's head is under the pointer, or dragged.
    pub(super) fn playhead_lit(&self, context: &App) -> bool {
        match (&self.timeline.drag, self.timeline.hover) {
            (TimelineDrag::None, Some((local_x, local_y))) => {
                self.on_playhead_head(local_x, local_y, context)
            }
            _ => false,
        }
    }

    /// The loop's part being dragged, or under the pointer.
    pub(super) fn loop_lit(&self, context: &App) -> Option<LoopGrab> {
        match (&self.timeline.drag, self.timeline.hover) {
            (TimelineDrag::Loop { grab, .. }, _) => Some(*grab),
            (TimelineDrag::None, Some((local_x, local_y)))
                if local_y < LOOP_HEIGHT && !self.on_playhead_head(local_x, local_y, context) =>
            {
                self.loop_grab_at(local_x, context)
            }
            _ => None,
        }
    }

    /// The loop's part at `local_x`: an end, or the bar between them.
    pub(super) fn loop_grab_at(&self, local_x: f32, context: &App) -> Option<LoopGrab> {
        let (start, end) = self.state(context).loop_range();
        let viewport = self.viewport();
        let (start_x, end_x) = (
            self.state(context).grid.beats_to_x(start, viewport),
            self.state(context).grid.beats_to_x(end, viewport),
        );
        let (to_start, to_end) = ((local_x - start_x).abs(), (local_x - end_x).abs());
        if to_start.min(to_end) <= GRIP_WIDTH {
            Some(if to_start <= to_end {
                LoopGrab::Start
            } else {
                LoopGrab::End
            })
        } else if start_x < local_x && local_x < end_x {
            Some(LoopGrab::Move {
                offset: (local_x - start_x) as f64
                    / self.state(context).grid.pixels_per_beat() as f64,
            })
        } else {
            None
        }
    }

    /// Follows a drag of `grab` to `local_x`.
    pub(super) fn drag_loop(&mut self, grab: LoopGrab, local_x: f32, alt: bool, context: &mut App) {
        let (start, end) = self.state(context).loop_range();
        let snapped = self.snapped(local_x, alt, context);
        let step = self.state(context).grid.snap_step();
        match grab {
            LoopGrab::Start => self
                .state_mut(context)
                .set_loop_range(snapped.min(end - step), end),
            LoopGrab::End => self
                .state_mut(context)
                .set_loop_range(start, snapped.max(start + step)),
            LoopGrab::Move { offset } => {
                let grid = &self.state(context).grid;
                let new_start = grid
                    .snap(self.beats_at(local_x, context) - offset, alt)
                    .max(BeatPos::ZERO);
                self.state_mut(context)
                    .set_loop_range(new_start, new_start + (end - start));
            }
            LoopGrab::Create { anchor } => {
                if snapped != anchor {
                    self.state_mut(context).set_loop_range(anchor, snapped);
                    self.state_mut(context).set_looping(true);
                }
            }
        }
    }
}

/// The loop bar: the accent while looping, dim otherwise, with the part
/// under the pointer or dragged (an end, or the whole bar) lit.
pub(super) fn paint_loop(scene: &Scene, canvas: &Canvas, window: &mut Window) {
    let theme = &scene.theme;
    let (color, on_color) = if scene.looping {
        (theme.accent, theme.text_on_accent)
    } else {
        (theme.text_disabled, theme.bg_deep)
    };
    let (start_x, end_x) = scene.loop_x;
    let bar = canvas.rect(start_x, INSET, end_x - start_x, LOOP_HEIGHT - 2. * INSET);
    let radius = px(bar.height / 2.);
    let moved = matches!(scene.loop_lit, Some(LoopGrab::Move { .. }));
    let alpha = if moved { MOVED_ALPHA } else { BODY_ALPHA };
    let body = with_alpha(color, (f32::from(color.a) * alpha) as u8);
    window.paint_quad(gpui::fill(bar.bounds(), hsla(body)).corner_radii(radius));

    // Too short for caps: just the bar.
    if bar.width < 4. * GRIP_WIDTH {
        return;
    }
    let caps = [
        (true, scene.loop_lit == Some(LoopGrab::Start)),
        (false, scene.loop_lit == Some(LoopGrab::End)),
    ];
    for (is_start, grabbed) in caps {
        let width = if grabbed { GRIP_WIDTH + 2. } else { GRIP_WIDTH };
        let (left, corners) = if is_start {
            let rounded = Corners {
                top_left: radius,
                bottom_left: radius,
                ..Corners::default()
            };
            (bar.left, rounded)
        } else {
            let rounded = Corners {
                top_right: radius,
                bottom_right: radius,
                ..Corners::default()
            };
            (bar.right() - width, rounded)
        };
        let cap = Rect { left, width, ..bar };
        window.paint_quad(gpui::fill(cap.bounds(), hsla(color)).corner_radii(corners));
        let middle = left + width / 2.;
        for offset in [-1., 1.] {
            let grip = Rect {
                left: middle + offset - 0.5,
                top: cap.top + 2.5,
                width: 1.,
                height: cap.height - 5.,
            };
            window.paint_quad(gpui::fill(grip.bounds(), hsla(on_color)));
        }
    }
}

/// The playhead from the ruler's top down to the footer, with its head on
/// the loop lane: the accent while under the pointer or dragged.
pub(super) fn paint_playhead(scene: &Scene, canvas: &Canvas, window: &mut Window) {
    let theme = &scene.theme;
    let playhead_x = scene.playhead.round();
    if !(0.0..=canvas.width).contains(&playhead_x) {
        return;
    }
    let lanes = canvas.lanes();
    let line = canvas.rect(
        playhead_x - PLAYHEAD_WIDTH / 2.,
        0.,
        PLAYHEAD_WIDTH,
        RULER_HEIGHT + lanes.height,
    );
    let line_color = with_alpha(theme.playhead, PLAYHEAD_ALPHA);
    window.paint_quad(gpui::fill(line.bounds(), hsla(line_color)));

    let head = canvas.rect(playhead_x - HEAD_WIDTH / 2., 0., HEAD_WIDTH, HEAD_HEIGHT);
    let shoulder = head.top + HEAD_HEIGHT * HEAD_SHOULDER;
    let mut path = PathBuilder::fill();
    path.move_to(point(px(head.left), px(head.top)));
    path.line_to(point(px(head.right()), px(head.top)));
    path.line_to(point(px(head.right()), px(shoulder)));
    path.line_to(point(px(head.left + HEAD_WIDTH / 2.), px(head.bottom())));
    path.line_to(point(px(head.left), px(shoulder)));
    path.close();
    let head_color = if scene.playhead_lit {
        theme.accent
    } else {
        theme.playhead
    };
    if let Ok(path) = path.build() {
        window.paint_path(path, hsla(head_color));
    }
}
