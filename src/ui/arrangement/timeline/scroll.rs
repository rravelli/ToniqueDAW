//! Keeping edits in view: scrolling while dragging near the edges of the
//! timeline, and revealing what an edit put off screen.

use egui::{Pos2, Rect, Ui, Vec2, vec2};

use crate::{
    core::{state::ProjectState, track::DEFAULT_TRACK_HEIGHT},
    ui::arrangement::row_layout::RowLayout,
};

/// How close to the left/right edges dragging starts scrolling, in points.
const EDGE_X: f32 = 16.;
/// Vertical scrolling only starts past the top/bottom edges: clips are
/// dragged by their header, so the pointer sits near the top edge whenever
/// a clip on the first visible track is dragged. Speed ramps over this
/// many points past the edge.
const EDGE_Y: f32 = 16.;
/// Scroll speed at full ramp, in points per second. It keeps growing
/// further out, up to 3×.
const SPEED: f32 = 600.;
/// Room kept around what gets revealed, in points.
const REVEAL_MARGIN: f32 = 40.;

/// Scroll while a drag holds the pointer near or past an edge of
/// `viewport`. Keeps repainting while it scrolls, as the pointer may be
/// still.
pub fn autoscroll(
    ui: &Ui,
    state: &mut ProjectState,
    layout: &RowLayout,
    viewport: Rect,
    pointer: Pos2,
) {
    let velocity = vec2(
        edge_speed(pointer.x, (viewport.left(), viewport.right()), EDGE_X),
        edge_speed(
            pointer.y,
            (viewport.top() - EDGE_Y, viewport.bottom() + EDGE_Y),
            EDGE_Y,
        ),
    );
    if velocity == Vec2::ZERO {
        return;
    }
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    // Allow scrolling one track past the last, where drops create a track.
    let tracks_height = layout.height();
    let max_y = (tracks_height + DEFAULT_TRACK_HEIGHT - viewport.height())
        .max(state.grid.offset.y)
        .max(0.);
    let offset = &mut state.grid.offset;
    offset.x = (offset.x + velocity.x * SPEED * dt).max(0.);
    offset.y = (offset.y + velocity.y * SPEED * dt).clamp(0., max_y);
    ui.ctx().request_repaint();
}

/// Scroll horizontally so `start..end` (beats) is in view. When it doesn't
/// fit, show its start.
pub fn reveal(ui: &Ui, state: &mut ProjectState, viewport: Rect, (start, end): (f32, f32)) {
    let ppb = state.grid.pixels_per_beat();
    let (start, end) = (start * ppb, end * ppb);
    let offset = state.grid.offset.x;
    let mut target = offset;
    if end > offset + viewport.width() - REVEAL_MARGIN {
        target = end + REVEAL_MARGIN - viewport.width();
    }
    if start < target + REVEAL_MARGIN {
        target = start - REVEAL_MARGIN;
    }
    let target = target.max(0.);
    if target != offset {
        state.grid.offset.x = target;
        ui.ctx().request_repaint();
    }
}

/// -1..0 inside the zone at the start of `range`, 0..1 inside the one at
/// its end, beyond (up to ±3) past the edges, 0 elsewhere.
fn edge_speed(pos: f32, range: (f32, f32), edge: f32) -> f32 {
    let (min, max) = range;
    if pos < min + edge {
        -((min + edge - pos) / edge).min(3.)
    } else if pos > max - edge {
        ((pos - (max - edge)) / edge).min(3.)
    } else {
        0.
    }
}

#[cfg(test)]
mod tests {
    use super::edge_speed;

    #[test]
    fn speed_grows_towards_and_past_the_edges() {
        let range = (0., 100.);
        assert_eq!(edge_speed(50., range, 10.), 0.);
        assert_eq!(edge_speed(95., range, 10.), 0.5);
        assert_eq!(edge_speed(110., range, 10.), 2.);
        assert_eq!(edge_speed(500., range, 10.), 3.);
        assert_eq!(edge_speed(5., range, 10.), -0.5);
        assert_eq!(edge_speed(-500., range, 10.), -3.);
    }
}
