use std::time::Duration;

use egui::{Rect, Vec2};
use tonique_engine::time::BeatPos;

const DEFAULT_THRESHOLD: f64 = 0.3;
/// How close (in points) edits snap to clip edges, loop edges and the edit
/// cursor. They win over the grid when in reach.
pub const TARGET_REACH: f32 = 8.;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GridResolution {
    SixTeenBar,
    FourBar,
    Bar,     // 1 line per bar
    Beat,    // 1 line per beat
    Quarter, // 4 lines per beat
    Height,  // 8 lines per beat
}

impl GridResolution {
    /// How many divisions per beat
    pub fn divisions_per_beat(&self, beats_per_bar: usize) -> f32 {
        match self {
            GridResolution::SixTeenBar => 1.0 / (beats_per_bar * 16) as f32,
            GridResolution::FourBar => 1.0 / (beats_per_bar * 4) as f32,
            GridResolution::Bar => 1.0 / (beats_per_bar as f32), // 1 line per bar (4 beats)
            GridResolution::Beat => 1.0,                         // 1 line per beat
            GridResolution::Quarter => 4.0,                      // 4 lines per beat
            GridResolution::Height => 8.0,
        }
    }

    pub fn step_size_secs(&self) -> f32 {
        match self {
            GridResolution::SixTeenBar => 30.0,
            GridResolution::FourBar => 30.0,
            GridResolution::Bar => 10.0,    // 1 line per bar (4 beats)
            GridResolution::Beat => 10.0,   // 1 line per beat
            GridResolution::Quarter => 1.0, // 4 lines per beat
            GridResolution::Height => 1.0,
        }
    }
}

pub struct GridService {
    pixels_per_beat: f32,
    beats_per_bar: usize,
    resolution: GridResolution,
    min_pixels_per_beat: f32,
    max_pixels_per_beat: f32,
    pub offset: Vec2,
    /// Whether the view may scroll before beat 0 (grid lines and labels
    /// still start at 0).
    negative_offset: bool,
}

impl GridService {
    pub fn new() -> Self {
        Self {
            pixels_per_beat: 10.,
            beats_per_bar: 4,
            resolution: GridResolution::Beat,
            min_pixels_per_beat: 0.5,
            max_pixels_per_beat: 5000.,
            offset: Vec2::ZERO,
            negative_offset: false,
        }
    }
    pub fn beats_per_bar(&self) -> usize {
        self.beats_per_bar
    }
    /// Which lines the grid shows at this zoom.
    pub fn resolution(&self) -> GridResolution {
        self.resolution
    }
    /// Spacing between grid lines, in beats.
    pub fn step_beats(&self) -> f64 {
        1.0 / self.resolution.divisions_per_beat(self.beats_per_bar) as f64
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
    /// clip in a grid counted from its start.
    pub fn with_negative_offset(mut self) -> Self {
        self.negative_offset = true;
        self
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
}
