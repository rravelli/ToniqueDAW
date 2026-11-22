//! Scrolling and zooming the timeline with the wheel, eased as in the egui
//! app ([`SmoothScroll`]): Ctrl zooms around the pointer, Shift (or a
//! sideways swipe) scrolls across, otherwise the rows scroll, from the
//! timeline or the track list alike. Also keeping edits in view: scrolling
//! while a drag holds the pointer near an edge, and revealing what an
//! edit put off screen; following the playhead during playback; and how
//! far the scrollbars go.

use crate::model::HoldsProject;
use crate::timeline::Arrangement;
use std::time::{Duration, Instant};

use tonique_engine::time::BeatPos;

use gpui::{App, Context, ScrollEvent, Window};

use crate::{
    core::{state::PlaybackState, track::DEFAULT_TRACK_HEIGHT},
    smooth_scroll::{SmoothScroll, WheelDelta},
    timeline::{
        RULER_HEIGHT,
        rows::{Row, rows, rows_height},
    },
    widgets::{ScrollSpan, ScrollbarGrab},
};

/// How close to the left and right edges a drag starts scrolling, in
/// pixels.
const EDGE_X: f32 = 16.;
/// Up and down, scrolling only starts past the edges: clips are dragged by
/// their header, near the top edge on the first row shown. Speed ramps
/// over this many pixels past it.
const EDGE_Y: f32 = 16.;
/// Scroll speed at full ramp, in pixels per second; it keeps growing
/// further out, up to 3×.
const EDGE_SPEED: f32 = 600.;
/// Room kept around what gets revealed, in pixels.
const REVEAL_MARGIN: f32 = 40.;
/// The longest step an autoscroll frame takes, in case frames stalled.
const MAX_FRAME: Duration = Duration::from_millis(100);
/// Bars past the arrangement's end the horizontal scrollbar reaches, as
/// room to add more.
const SLACK_BARS: f64 = 16.;

/// What the wheel has left to scroll and zoom.
#[derive(Default)]
pub struct TimelineScroll {
    /// When a drag last scrolled the timeline.
    autoscrolled: Option<Instant>,
    rows: SmoothScroll,
    across: SmoothScroll,
    zoom: SmoothScroll,
    /// Where the zoom is centred, local to the timeline.
    zoom_anchor: f32,
    /// Playing as of the last frame: following resumes when play starts.
    was_playing: bool,
    /// Scrolled across by hand since play started: the view stays put.
    follow_paused: bool,
    /// The scrollbars along the lanes' bottom and down the track list.
    pub across_bar: ScrollbarGrab,
    pub rows_bar: ScrollbarGrab,
}

impl Arrangement {
    pub(super) fn on_timeline_scroll(
        &mut self,
        event: &ScrollEvent,
        _: &mut Window,
        context: &mut Context<Self>,
    ) {
        let wheel = WheelDelta::of(event.delta);
        let (local_x, _) = self.local(event.position);
        let scroll = &mut self.timeline.scroll;
        if event.modifiers.control || event.modifiers.platform {
            scroll.zoom_anchor = local_x;
            let now = scroll.zoom.push(-wheel.vertical, wheel.eased);
            self.zoom_by(now, context);
        } else if event.modifiers.shift || wheel.horizontal.abs() > wheel.vertical.abs() {
            // A wheel turned with Shift held scrolls across too.
            let amount = if wheel.horizontal.abs() > wheel.vertical.abs() {
                wheel.horizontal
            } else {
                wheel.vertical
            };
            let now = scroll.across.push(amount, wheel.eased);
            scroll.follow_paused = true;
            self.scroll_across(now, context);
        } else {
            self.scroll_rows_by_wheel(wheel, context);
        }
        context.notify();
    }

    /// The rows scrolled by the wheel: from the timeline or the track list.
    pub fn scroll_rows_by_wheel(&mut self, wheel: WheelDelta, context: &mut App) {
        let now = self.timeline.scroll.rows.push(wheel.vertical, wheel.eased);
        self.scroll_rows(now, context);
    }

    /// Applies this frame's part of the eased scrolling and zooming.
    /// Returns whether some is left, so frames must keep coming.
    pub fn step_scrolling(&mut self, context: &mut App) -> bool {
        let now = Instant::now();
        let scroll = &mut self.timeline.scroll;
        let (rows, across, zoom) = (
            scroll.rows.step(now),
            scroll.across.step(now),
            scroll.zoom.step(now),
        );
        self.scroll_rows(rows, context);
        self.scroll_across(across, context);
        self.zoom_by(zoom, context);
        let scroll = &self.timeline.scroll;
        scroll.rows.is_active() || scroll.across.is_active() || scroll.zoom.is_active()
    }

    /// Scrolls the rows (the timeline's lanes and the track headers) by
    /// `delta_y`, as far as the last row and a row more. Past an end, what
    /// the wheel has left is dropped.
    pub fn scroll_rows(&mut self, delta_y: f32, context: &mut App) {
        if delta_y == 0. {
            return;
        }
        let rows = rows(self.state(context));
        let lanes_height = self.lanes_bottom(context) - RULER_HEIGHT;
        let max = (rows_height(&rows) + DEFAULT_TRACK_HEIGHT - lanes_height).max(0.);
        let offset = &mut self.state_mut(context).grid.offset.y;
        let to = (*offset - delta_y).clamp(0., max);
        if to == *offset {
            self.timeline.scroll.rows.stop();
        }
        *offset = to;
    }

    /// The rows' scroll, as far as [`Self::scroll_rows`] goes.
    pub fn rows_span(&self, rows: &[Row], context: &App) -> ScrollSpan {
        let view = self.lanes_bottom(context) - RULER_HEIGHT;
        let content = rows_height(rows) + DEFAULT_TRACK_HEIGHT;
        ScrollSpan {
            offset: self.state(context).grid.offset.y,
            max: (content - view).max(0.),
            view,
        }
    }

    /// The scroll across: the arrangement and some room to add more, never
    /// short of what's in view.
    pub fn across_span(&self, context: &App) -> ScrollSpan {
        let grid = &self.state(context).grid;
        let end = self.state(context).arrangement_end().0 + SLACK_BARS * grid.beats_per_bar();
        let view = self.viewport().right;
        let offset = grid.offset.x;
        ScrollSpan {
            offset,
            max: (grid.beats_to_width(end) - view).max(offset).max(0.),
            view,
        }
    }

    /// Scrolled across from the scrollbar: following the playhead pauses,
    /// as with the wheel.
    pub fn set_across(&mut self, offset: f32, context: &mut App) {
        self.state_mut(context).grid.offset.x = offset.max(0.);
        self.timeline.scroll.across.stop();
        self.timeline.scroll.follow_paused = true;
    }

    pub fn set_rows(&mut self, offset: f32, context: &mut App) {
        self.state_mut(context).grid.offset.y = offset.max(0.);
        self.timeline.scroll.rows.stop();
    }

    /// Scrolls across by `delta_x`, not before the start.
    fn scroll_across(&mut self, delta_x: f32, context: &mut App) {
        if delta_x == 0. {
            return;
        }
        let offset = &mut self.state_mut(context).grid.offset.x;
        let to = (*offset - delta_x).max(0.);
        if to == *offset {
            self.timeline.scroll.across.stop();
        }
        *offset = to;
    }

    /// Scrolls while a drag holds the pointer near or past an edge of the
    /// lanes, following it again over what scrolled under it. Returns
    /// whether it scrolled.
    pub fn autoscroll(&mut self, context: &mut App) -> bool {
        let now = Instant::now();
        let last = self.timeline.scroll.autoscrolled.take();
        let Some((position, modifiers)) = self.timeline.pointer else {
            return false;
        };
        if !self.timeline.drag.scrolls() {
            return false;
        }
        let (local_x, local_y) = self.local(position);
        let speed_x = edge_speed(local_x, (0., self.viewport().right), EDGE_X);
        let lanes = (RULER_HEIGHT - EDGE_Y, self.lanes_bottom(context) + EDGE_Y);
        let speed_y = edge_speed(local_y, lanes, EDGE_Y);
        if speed_x == 0. && speed_y == 0. {
            return false;
        }
        self.timeline.scroll.autoscrolled = Some(now);
        // The first frame near the edge only starts the clock.
        let Some(last) = last else { return true };
        let seconds = now.duration_since(last).min(MAX_FRAME).as_secs_f32();
        let step = EDGE_SPEED * seconds;
        self.scroll_across(-speed_x * step, context);
        self.scroll_rows(-speed_y * step, context);
        self.follow_timeline_drag(position, &modifiers, context);
        true
    }

    /// Scrolls across so `start..end` is in view; when it doesn't fit, its
    /// start.
    pub fn reveal(&mut self, (start, end): (BeatPos, BeatPos), context: &mut App) {
        let grid = &self.state(context).grid;
        let (start, end) = (grid.beats_to_width(start.0), grid.beats_to_width(end.0));
        let width = self.viewport().right;
        let offset = grid.offset.x;
        let mut target = offset;
        if end > offset + width - REVEAL_MARGIN {
            target = end + REVEAL_MARGIN - width;
        }
        if start < target + REVEAL_MARGIN {
            target = start - REVEAL_MARGIN;
        }
        self.state_mut(context).grid.offset.x = target.max(0.);
    }

    /// Scrolls to what an edit just selected: copies, pasted or moved clips.
    pub fn reveal_selection(&mut self, context: &mut App) {
        if let Some(range) = self.state(context).selection_range() {
            self.reveal(range, context);
        }
    }

    /// Keeps the playhead in the middle of the view during playback, when
    /// following it. Near the start of the arrangement the view stays put
    /// and the playhead moves towards the middle. Not under a drag
    /// (`dragging`), nor once scrolled across by hand until play starts
    /// again.
    pub fn follow_playhead(&mut self, dragging: bool, context: &mut App) {
        let playing = self.state(context).playback_state() == PlaybackState::Playing;
        let scroll = &mut self.timeline.scroll;
        if playing && !scroll.was_playing {
            scroll.follow_paused = false;
        }
        scroll.was_playing = playing;
        if !playing
            || !self.project.read(context).state.follow_playhead()
            || scroll.follow_paused
            || dragging
        {
            return;
        }
        let playhead_x = self
            .state(context)
            .grid
            .beats_to_width(self.state(context).playhead().0);
        let width = self.viewport().right;
        self.state_mut(context).grid.offset.x = (playhead_x - width / 2.).max(0.);
    }

    /// Zooms by `amount` around where the wheel was turned.
    fn zoom_by(&mut self, amount: f32, context: &mut App) {
        if amount == 0. {
            return;
        }
        let anchor = self.timeline.scroll.zoom_anchor;
        self.state_mut(context)
            .grid
            .zoom_around(amount, anchor, self.viewport());
    }
}

/// -1..0 inside the edge at the start of `range`, 0..1 inside the one at
/// its end, beyond (up to ±3) past them, 0 elsewhere.
fn edge_speed(position: f32, (min, max): (f32, f32), edge: f32) -> f32 {
    if position < min + edge {
        -((min + edge - position) / edge).min(3.)
    } else if position > max - edge {
        ((position - (max - edge)) / edge).min(3.)
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
