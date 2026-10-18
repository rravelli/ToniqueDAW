use std::time::Duration;

use egui::{Rect, Vec2};
use tonique_engine::time::{BeatPos, TimeSignature};

const DEFAULT_THRESHOLD: f64 = 0.3;
/// How close (in points) edits snap to clip edges, loop edges and the edit
/// cursor. They win over the grid when in reach.
pub const TARGET_REACH: f32 = 8.;

/// Which lines the grid shows, from the coarsest to the finest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GridResolution {
    SixTeenBar,
    FourBar,
    Bar,
    /// The beats the time signature counts: quarters in 3/4, eighths in 6/8.
    Beat,
    /// Sixteenth notes.
    Quarter,
    /// Thirty-second notes.
    Height,
}

impl GridResolution {
    /// Spacing between lines, in quarter-note beats.
    pub fn step_beats(&self, signature: TimeSignature) -> f64 {
        match self {
            GridResolution::SixTeenBar => 16.0 * signature.beats_per_bar(),
            GridResolution::FourBar => 4.0 * signature.beats_per_bar(),
            GridResolution::Bar => signature.beats_per_bar(),
            GridResolution::Beat => signature.beat_length(),
            GridResolution::Quarter => 0.25,
            GridResolution::Height => 0.125,
        }
    }

    pub fn step_size_secs(&self) -> f32 {
        match self {
            GridResolution::SixTeenBar => 30.0,
            GridResolution::FourBar => 30.0,
            GridResolution::Bar => 10.0,
            GridResolution::Beat => 10.0,
            GridResolution::Quarter => 1.0,
            GridResolution::Height => 1.0,
        }
    }
}

/// What a grid line marks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GridLine {
    /// The start of a bar (numbered from 1).
    Bar(i64),
    /// A counted beat inside a bar (both numbered from 1).
    Beat { bar: i64, beat: u32 },
    /// A subdivision of a beat.
    Division,
}

pub struct GridService {
    pixels_per_beat: f32,
    /// A copy of the edit's, kept in sync by the project state.
    time_signature: TimeSignature,
    resolution: GridResolution,
    min_pixels_per_beat: f32,
    max_pixels_per_beat: f32,
    pub offset: Vec2,
    /// Whether the view may scroll before beat 0, with grid lines and
    /// labels there too (bars count down from -1).
    negative_offset: bool,
}

impl GridService {
    pub fn new() -> Self {
        Self {
            pixels_per_beat: 10.,
            time_signature: TimeSignature::default(),
            resolution: GridResolution::Beat,
            min_pixels_per_beat: 0.5,
            max_pixels_per_beat: 5000.,
            offset: Vec2::ZERO,
            negative_offset: false,
        }
    }
    pub fn set_time_signature(&mut self, signature: TimeSignature) {
        self.time_signature = signature;
    }
    /// Length of a bar, in quarter-note beats.
    pub fn beats_per_bar(&self) -> f64 {
        self.time_signature.beats_per_bar()
    }
    /// Which lines the grid shows at this zoom.
    pub fn resolution(&self) -> GridResolution {
        self.resolution
    }
    /// Spacing between grid lines, in beats.
    pub fn step_beats(&self) -> f64 {
        self.resolution.step_beats(self.time_signature)
    }
    /// What the grid line `index` steps from the start marks.
    pub fn line(&self, index: i64) -> GridLine {
        // Bars, beats and steps are all whole numbers of thirty-second
        // notes, so counting in those is exact.
        let units = |beats: f64| (beats * 8.).round() as i64;
        let bar = units(self.beats_per_bar()).max(1);
        let beat = units(self.time_signature.beat_length()).max(1);
        let position = index * units(self.step_beats());
        let in_bar = position.rem_euclid(bar);
        // From 1, and before beat 0 from -1 down: there's no bar 0.
        let number = match position.div_euclid(bar) {
            n if n >= 0 => n + 1,
            n => n,
        };
        if in_bar == 0 {
            GridLine::Bar(number)
        } else if in_bar % beat == 0 {
            GridLine::Beat {
                bar: number,
                beat: (in_bar / beat) as u32 + 1,
            }
        } else {
            GridLine::Division
        }
    }
    /// The nearest grid line, however far.
    pub fn snap_to_step(&self, beats: BeatPos) -> BeatPos {
        let step = self.step_beats();
        BeatPos((beats.0 / step).round() * step)
    }
    pub fn pixels_per_beat(&self) -> f32 {
        self.pixels_per_beat
    }
    /// Convert a time duration to an actual screen width
    pub fn duration_to_width(&self, duration: Duration, bpm: f32) -> f32 {
        duration.as_secs_f32() / 60.0 * bpm * self.pixels_per_beat
    }
    /// Position in beats to actual screen x position
    pub fn beats_to_x(&self, beats: BeatPos, viewport: Rect) -> f32 {
        viewport.left() + beats.0 as f32 * self.pixels_per_beat - self.offset.x
    }
    /// Actual x position to beats position
    pub fn x_to_beats(&self, x: f32, viewport: Rect) -> BeatPos {
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

    /// Zoom from a scroll delta, keeping the beat under `cursor_x` in place.
    pub fn zoom_around(&mut self, delta: f32, cursor_x: f32, viewport: Rect) {
        self.zoom_by((1.0 - delta * 0.007).clamp(0., 2.0), cursor_x, viewport);
    }

    /// Multiply the zoom by `factor`, keeping the beat under `cursor_x` in place.
    pub fn zoom_by(&mut self, factor: f32, cursor_x: f32, viewport: Rect) {
        let old_ppb = self.pixels_per_beat;
        let old_offset_x = self.offset.x;

        // Beat that the cursor is pointing to (world position)
        let beat_under_cursor = (cursor_x + old_offset_x - viewport.left()) / old_ppb;

        // Apply zoom (clamped)
        self.pixels_per_beat = (self.pixels_per_beat * factor)
            .clamp(self.min_pixels_per_beat, self.max_pixels_per_beat);
        self.update_resolution();

        // New offset that keeps the same beat under the cursor
        let new_ppb = self.pixels_per_beat;
        let new_offset_x = beat_under_cursor * new_ppb - (cursor_x - viewport.left());

        self.offset.x = self.bound_offset(new_offset_x);
    }

    /// Zoom and scroll so `start..end` (in beats) spans `width` points.
    pub fn fit(&mut self, start: BeatPos, end: BeatPos, width: f32) {
        if end > start && width > 0. {
            self.pixels_per_beat = (width / (end - start) as f32)
                .clamp(self.min_pixels_per_beat, self.max_pixels_per_beat);
            self.update_resolution();
        }
        self.offset.x = self.bound_offset(self.beats_to_width(start.0));
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

    fn bound_offset(&self, offset: f32) -> f32 {
        if self.negative_offset {
            offset
        } else {
            offset.max(0.)
        }
    }

    /// Snap `beats` to the nearest of `targets` within [`TARGET_REACH`]
    /// points, or else to the grid. Returns the snapped position and
    /// whether it came from `targets`.
    pub fn snap_to_targets(&self, beats: BeatPos, targets: &[BeatPos]) -> Option<(BeatPos, bool)> {
        let reach = self.width_to_beats(TARGET_REACH);
        let target = targets
            .iter()
            .copied()
            .filter(|t| (*t - beats).abs() <= reach)
            .min_by(|a, b| (*a - beats).abs().total_cmp(&(*b - beats).abs()));
        match target {
            Some(t) => Some((t, true)),
            None => self.snap_at_grid_option(beats).map(|g| (g, false)),
        }
    }

    pub fn snap_at_grid(&self, beats: BeatPos) -> BeatPos {
        self.snap_at_grid_with_threshold(beats, DEFAULT_THRESHOLD)
            .unwrap_or(beats)
    }

    pub fn snap_at_grid_option(&self, beats: BeatPos) -> Option<BeatPos> {
        self.snap_at_grid_with_threshold(beats, DEFAULT_THRESHOLD)
    }

    pub fn snap_at_grid_with_threshold(&self, beats: BeatPos, threshold: f64) -> Option<BeatPos> {
        let step = self.step_beats();
        let nearest_position = self.snap_to_step(beats);
        if (beats - nearest_position).abs() < step * threshold {
            Some(nearest_position)
        } else {
            None
        }
    }

    fn update_resolution(&mut self) {
        let new_resolution = if self.pixels_per_beat < 1.0 {
            GridResolution::SixTeenBar
        } else if self.pixels_per_beat < 4.0 {
            GridResolution::FourBar
        } else if self.pixels_per_beat < 15.0 {
            GridResolution::Bar
        } else if self.pixels_per_beat < 80.0 {
            GridResolution::Beat
        } else if self.pixels_per_beat < 300.0 {
            GridResolution::Quarter
        } else {
            GridResolution::Height
        };
        self.resolution = new_resolution;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_shows_exactly_the_range() {
        let mut grid = GridService::new();
        let viewport = Rect::from_min_size(egui::pos2(100., 0.), egui::vec2(400., 50.));
        grid.fit(BeatPos(8.), BeatPos(16.), 400.);
        assert_eq!(grid.pixels_per_beat(), 50.);
        assert_eq!(grid.beats_to_x(BeatPos(8.), viewport), 100.);
        assert_eq!(grid.beats_to_x(BeatPos(16.), viewport), 500.);
        assert_eq!(grid.step_beats(), 1.); // resolution follows the zoom

        // Before beat 0 only when allowed.
        grid.fit(BeatPos(-2.), BeatPos(6.), 400.);
        assert_eq!(grid.offset.x, 0.);
        let mut grid = GridService::new().with_negative_offset();
        grid.fit(BeatPos(-2.), BeatPos(6.), 400.);
        assert_eq!(grid.beats_to_x(BeatPos(-2.), viewport), 100.);
    }

    fn grid(numerator: u32, denominator: u32, resolution: GridResolution) -> GridService {
        let mut grid = GridService::new();
        grid.set_time_signature(TimeSignature::new(numerator, denominator).unwrap());
        grid.resolution = resolution;
        grid
    }

    fn lines(grid: &GridService, count: i64) -> Vec<GridLine> {
        (0..count).map(|i| grid.line(i)).collect()
    }

    #[test]
    fn beats_and_bars_in_four_four() {
        let grid = grid(4, 4, GridResolution::Beat);
        assert_eq!(grid.step_beats(), 1.0);
        assert_eq!(
            lines(&grid, 5),
            [
                GridLine::Bar(1),
                GridLine::Beat { bar: 1, beat: 2 },
                GridLine::Beat { bar: 1, beat: 3 },
                GridLine::Beat { bar: 1, beat: 4 },
                GridLine::Bar(2),
            ]
        );
    }

    /// 7/8 counts eighths: bars are 3.5 quarter notes.
    #[test]
    fn odd_meters_land_bars_between_quarters() {
        let grid = grid(7, 8, GridResolution::Beat);
        assert_eq!(grid.step_beats(), 0.5);
        assert_eq!(grid.line(6), GridLine::Beat { bar: 1, beat: 7 });
        assert_eq!(grid.line(7), GridLine::Bar(2));
        // Sixteenths: every other line is an eighth.
        let grid = self::grid(7, 8, GridResolution::Quarter);
        assert_eq!(grid.line(13), GridLine::Division);
        assert_eq!(grid.line(14), GridLine::Bar(2));
        assert_eq!(grid.line(16), GridLine::Beat { bar: 2, beat: 2 });
    }

    #[test]
    fn compound_meters_count_eighths() {
        let grid = grid(6, 8, GridResolution::Quarter);
        // A sixteenth grid: eighth 4 of bar 1 is at 1.5 quarters.
        assert_eq!(grid.line(6), GridLine::Beat { bar: 1, beat: 4 });
        assert_eq!(grid.line(12), GridLine::Bar(2));
    }

    /// Zoomed out, each line spans several bars and is numbered by its bar.
    #[test]
    fn multi_bar_lines_number_their_bars() {
        let grid = grid(3, 4, GridResolution::FourBar);
        assert_eq!(grid.step_beats(), 12.0);
        assert_eq!(
            lines(&grid, 3),
            [GridLine::Bar(1), GridLine::Bar(5), GridLine::Bar(9)]
        );
    }

    #[test]
    fn snapping_follows_the_signature() {
        let grid = grid(7, 8, GridResolution::Bar);
        assert_eq!(grid.snap_to_step(BeatPos(4.0)), BeatPos(3.5));
        assert_eq!(grid.snap_to_step(BeatPos(5.5)), BeatPos(7.0));
    }

    /// Before beat 0 bars count down from -1: there's no bar 0.
    #[test]
    fn bars_before_the_start_count_down_from_minus_one() {
        let grid = grid(4, 4, GridResolution::Bar);
        assert_eq!(grid.line(0), GridLine::Bar(1));
        assert_eq!(grid.line(-1), GridLine::Bar(-1));
        assert_eq!(grid.line(-2), GridLine::Bar(-2));
        let grid = self::grid(4, 4, GridResolution::Beat);
        assert_eq!(grid.line(-1), GridLine::Beat { bar: -1, beat: 4 });
    }
}
