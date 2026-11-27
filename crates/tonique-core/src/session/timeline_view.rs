//! How a view of time is zoomed and scrolled: the arrangement's, or a
//! clip editor's. It belongs to the view showing it, never to the
//! project: it changes on every scroll and is never saved. The musical
//! grid drawn over it is the project's [`super::grid::Grid`], whose
//! adaptive steps follow this zoom.

use std::time::Duration;

use tonique_engine::time::BeatPos;

/// Steps of the time labels, in seconds.
const TIME_STEPS: [f64; 21] = [
    0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1., 2., 5., 10., 15., 30., 60., 120.,
    300., 600., 1800., 3600.,
];
/// Estimated width of a digit or dot of a ruler label, and the room left
/// between two labels.
pub const LABEL_CHAR_WIDTH: f32 = 6.;
pub const LABEL_GAP: f32 = 10.;

/// Where a view is across the screen: what beats are mapped onto. The UI
/// implements it for its toolkit's rectangles.
pub trait Viewport: Copy {
    /// Screen x of its left edge.
    fn left(&self) -> f32;
    /// Screen x of its right edge.
    fn right(&self) -> f32;
}

/// A view's left and right edges on screen: a [`Viewport`] of its own, for
/// UIs whose rectangles the core can't know.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Span {
    pub left: f32,
    pub right: f32,
}

impl Viewport for Span {
    fn left(&self) -> f32 {
        self.left
    }
    fn right(&self) -> f32 {
        self.right
    }
}

/// How far a view is scrolled, in points: across (time) and down (rows).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Offset {
    pub x: f32,
    pub y: f32,
}

impl Offset {
    pub const ZERO: Offset = Offset { x: 0., y: 0. };
}

/// A view's zoom (points per beat) and scroll.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelineView {
    pixels_per_beat: f32,
    min_pixels_per_beat: f32,
    max_pixels_per_beat: f32,
    pub offset: Offset,
    /// Whether the view may scroll before beat 0, with grid lines and
    /// labels there too (bars count down from -1).
    negative_offset: bool,
}

impl Default for TimelineView {
    fn default() -> Self {
        Self::new()
    }
}

impl TimelineView {
    pub fn new() -> Self {
        Self {
            pixels_per_beat: 10.,
            min_pixels_per_beat: 0.05,
            max_pixels_per_beat: 5000.,
            offset: Offset::ZERO,
            negative_offset: false,
        }
    }

    /// A view at `pixels_per_beat`, unclamped.
    #[cfg(test)]
    pub(crate) fn zoomed(pixels_per_beat: f32) -> Self {
        Self {
            pixels_per_beat,
            ..Self::new()
        }
    }

    /// Let the view scroll before beat 0, e.g. to show what's before a
    /// clip in a grid counted from its start. The grid and its labels go
    /// on there.
    pub fn with_negative_offset(mut self) -> Self {
        self.negative_offset = true;
        self
    }

    /// Whether the view may show beats before 0 (see
    /// [`Self::with_negative_offset`]).
    pub fn allows_negative(&self) -> bool {
        self.negative_offset
    }

    pub fn pixels_per_beat(&self) -> f32 {
        self.pixels_per_beat
    }

    /// Convert a time duration to an actual screen width
    pub fn duration_to_width(&self, duration: Duration, bpm: f32) -> f32 {
        duration.as_secs_f32() / 60.0 * bpm * self.pixels_per_beat
    }

    /// The beats `rect` shows, none before 0 unless the view may go there.
    pub fn visible_beats(&self, rect: impl Viewport) -> (BeatPos, BeatPos) {
        self.visible_beats_in(rect, rect)
    }

    /// The beats `shown` covers, drawn in `viewport`'s coordinates, none
    /// before 0 unless the view may go there.
    pub fn visible_beats_in(
        &self,
        shown: impl Viewport,
        viewport: impl Viewport,
    ) -> (BeatPos, BeatPos) {
        let from = self.x_to_beats(shown.left(), viewport);
        let from = if self.negative_offset {
            from
        } else {
            from.max(BeatPos::ZERO)
        };
        (from, self.x_to_beats(shown.right(), viewport))
    }

    /// Position in beats to actual screen x position
    pub fn beats_to_x(&self, beats: BeatPos, viewport: impl Viewport) -> f32 {
        viewport.left() + beats.0 as f32 * self.pixels_per_beat - self.offset.x
    }
    /// Actual x position to beats position
    pub fn x_to_beats(&self, x: f32, viewport: impl Viewport) -> BeatPos {
        BeatPos(((x + self.offset.x - viewport.left()) / self.pixels_per_beat) as f64)
    }
    /// A width on screen, in beats.
    pub fn width_to_beats(&self, width: f32) -> f64 {
        (width / self.pixels_per_beat) as f64
    }
    /// A length in beats, as a width on screen.
    pub fn beats_to_width(&self, beats: f64) -> f32 {
        beats as f32 * self.pixels_per_beat
    }

    /// Seconds between time labels at `bpm`: the finest of 1, 2, 5, 10, 15,
    /// 30 s… (and fractions down to a millisecond) whose labels fit.
    pub fn time_step(&self, bpm: f32) -> f64 {
        let pixels_per_second = self.pixels_per_beat as f64 * bpm as f64 / 60.;
        TIME_STEPS
            .into_iter()
            .find(|step| {
                // `0:05`, or `0:01.250`.
                let chars = if *step < 1. { 8. } else { 5. };
                step * pixels_per_second >= chars * LABEL_CHAR_WIDTH as f64 + LABEL_GAP as f64
            })
            .unwrap_or(TIME_STEPS[TIME_STEPS.len() - 1])
    }

    /// Zoom from a scroll delta, keeping the beat under `cursor_x` in place.
    pub fn zoom_around(&mut self, delta: f32, cursor_x: f32, viewport: impl Viewport) {
        self.zoom_by((1.0 - delta * 0.007).clamp(0., 2.0), cursor_x, viewport);
    }

    /// Multiply the zoom by `factor`, keeping the beat under `cursor_x` in place.
    pub fn zoom_by(&mut self, factor: f32, cursor_x: f32, viewport: impl Viewport) {
        // The beat under the cursor, kept there.
        let beat_under_cursor = (cursor_x + self.offset.x - viewport.left()) / self.pixels_per_beat;
        self.pixels_per_beat = (self.pixels_per_beat * factor)
            .clamp(self.min_pixels_per_beat, self.max_pixels_per_beat);
        let offset = beat_under_cursor * self.pixels_per_beat - (cursor_x - viewport.left());
        self.offset.x = self.bound_offset(offset);
    }

    /// Zoom and scroll so `start..end` (in beats) spans `width` points.
    pub fn fit(&mut self, start: BeatPos, end: BeatPos, width: f32) {
        if end > start && width > 0. {
            self.pixels_per_beat = (width / (end - start) as f32)
                .clamp(self.min_pixels_per_beat, self.max_pixels_per_beat);
        }
        self.offset.x = self.bound_offset(self.beats_to_width(start.0));
    }

    fn bound_offset(&self, offset: f32) -> f32 {
        if self.negative_offset {
            offset
        } else {
            offset.max(0.)
        }
    }
}
