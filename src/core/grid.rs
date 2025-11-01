use std::time::Duration;

use egui::{Align2, Color32, FontId, Painter, Rect, Stroke, Vec2, pos2};

const DEFAULT_THRESHOLD: f32 = 0.3;

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
        }
    }
    pub fn pixels_per_beat(&self) -> f32 {
        self.pixels_per_beat
    }
    /// Convert a time duration to an actual screen width
    pub fn duration_to_width(&self, duration: Duration, bpm: f32) -> f32 {
        duration.as_secs_f32() / 60.0 * bpm * self.pixels_per_beat
    }
    /// Position in beats to actual screen x position
    pub fn beats_to_x(&self, beats: f32, viewport: Rect) -> f32 {
        viewport.left() + beats * self.pixels_per_beat - self.offset.x
    }
    /// Actual x position to beats position
    pub fn x_to_beats(&self, x: f32, viewport: Rect) -> f32 {
        (x + self.offset.x - viewport.left()) / self.pixels_per_beat
    }

    pub fn zoom_around(&mut self, delta: f32, cursor_x: f32, viewport: Rect) {
        let factor = (1.0 - delta * 0.007).clamp(0., 2.0);
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

        self.offset.x = new_offset_x.max(0.);
    }

    pub fn snap_at_grid(&self, beats: f32) -> f32 {
        self.snap_at_grid_with_threshold(beats, DEFAULT_THRESHOLD)
            .unwrap_or(beats)
    }

    pub fn snap_at_grid_option(&self, beats: f32) -> Option<f32> {
        self.snap_at_grid_with_threshold(beats, DEFAULT_THRESHOLD)
    }

    pub fn snap_at_grid_with_threshold(&self, beats: f32, threshold: f32) -> Option<f32> {
        let divisions = self.resolution.divisions_per_beat(self.beats_per_bar);
        let step = 1.0 / divisions; // spacing between grid lines (in beats)

        let nearest_position = (beats / step).round() * step;

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

    pub fn render_clip_grid(&self, painter: &Painter, viewport: Rect, rect: Rect, color: Color32) {
        if self.resolution.divisions_per_beat(self.beats_per_bar)
            <= GridResolution::Bar.divisions_per_beat(self.beats_per_bar)
        {
            return;
        }
        let divisions_per_beat = GridResolution::Bar.divisions_per_beat(self.beats_per_bar);

        let step = self.pixels_per_beat / divisions_per_beat; // pixel spacing between grid lines
        let clip_offset = self.offset.x + rect.left() - viewport.left();
        let mut step_index = (clip_offset / step).floor() as i32;
        let mut x = rect.left() - clip_offset.rem_euclid(step);
        while x < rect.right() {
            // Skip if out of bounds
            if step_index < 0 {
                step_index += 1;
                x += step;
                continue;
            }

            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1.0, color),
            );

            step_index += 1;
            x += step;
        }
    }

    pub fn render_grid(&self, painter: &Painter, viewport: Rect) {
        let divisions_per_beat = self.resolution.divisions_per_beat(self.beats_per_bar);
        let step = self.pixels_per_beat / divisions_per_beat; // pixel spacing between grid lines

        let mut step_index = (self.offset.x / step).floor() as i32;
        let right = viewport.right();
        let mut x = viewport.left() - self.offset.x.rem_euclid(step);

        let lines_per_bar = (self.beats_per_bar as f32 * divisions_per_beat).max(1.0) as i32;
        let lines_per_beat = divisions_per_beat.max(1.0) as i32;

        while x < right {
            // Skip if out of bounds
            if step_index < 0 {
                step_index += 1;
                x += step;
                continue;
            }
            let is_bar = step_index % lines_per_bar == 0;
            let is_major_beat = step_index % lines_per_beat == 0;

            let color = if is_bar {
                Color32::from_gray(90)
            } else if is_major_beat {
                Color32::from_gray(70)
            } else {
                Color32::from_gray(60)
            };

            painter.line_segment(
                [pos2(x, viewport.top()), pos2(x, viewport.bottom())],
                Stroke::new(if is_bar { 2.0 } else { 1.0 }, color),
            );

            step_index += 1;
            x += step;
        }
    }

    pub fn render_labels(&self, painter: &Painter, rect: Rect, bpm: f32) {
        let divisions_per_beat = self.resolution.divisions_per_beat(self.beats_per_bar);
        let step = self.pixels_per_beat / divisions_per_beat; // pixel spacing between grid lines

        let mut step_index = (self.offset.x / step).floor() as i32;
        let right = rect.right();
        let mut x = rect.left() - self.offset.x.rem_euclid(step);

        let lines_per_bar = (self.beats_per_bar as f32 * divisions_per_beat).max(1.0) as i32;
        let lines_per_beat = divisions_per_beat.max(1.0) as i32;

        while x < right {
            // Skip if out of bounds
            if step_index < 0 {
                step_index += 1;
                x += step;
                continue;
            }

            let is_bar = step_index % lines_per_bar == 0;
            let is_major_beat = step_index % lines_per_beat == 0;

            let bar_index = step_index.div_euclid(lines_per_bar) + 1;
            if is_bar {
                let text = format!("{}", bar_index);
                painter.text(
                    pos2(x + 3.0, rect.bottom() - 1.0),
                    Align2::LEFT_BOTTOM,
                    text,
                    FontId::new(8., egui::FontFamily::Monospace),
                    Color32::WHITE,
                );
                painter.line_segment(
                    [pos2(x, rect.bottom() - 8.0), pos2(x, rect.bottom())],
                    Stroke::new(2.0, Color32::from_gray(120)),
                );
            }
            let beat_index =
                step_index.div_euclid(lines_per_beat) % (self.beats_per_bar as i32) + 1;
            if divisions_per_beat >= GridResolution::Quarter.divisions_per_beat(self.beats_per_bar)
                && is_major_beat
            {
                let text = format!("{}.{}", bar_index, beat_index);
                painter.text(
                    pos2(x + 3.0, rect.bottom() - 1.0),
                    Align2::LEFT_BOTTOM,
                    text,
                    FontId::new(8., egui::FontFamily::Monospace),
                    Color32::WHITE,
                );
                painter.line_segment(
                    [pos2(x, rect.bottom() - 8.0), pos2(x, rect.bottom())],
                    Stroke::new(1.0, Color32::from_gray(120)),
                );
            }

            step_index += 1;
            x += step;
        }

        self.render_time_labels(painter, rect, bpm);
    }

    fn render_time_labels(&self, painter: &Painter, rect: Rect, bpm: f32) {
        let seconds_step = self.resolution.step_size_secs();
        let step = self.pixels_per_beat * bpm / 60. * seconds_step;
        let mut step_index = (self.offset.x / step).floor() as i32;
        let mut x = rect.left() - self.offset.x.rem_euclid(step);

        while x < rect.right() {
            // Skip if out of bounds
            if step_index < 0 {
                step_index += 1;
                x += step;
                continue;
            }
            let time = (step_index as f32 * seconds_step).floor() as i32;
            let seconds = time % 60;
            let minutes = (time / 60) % 60;
            let text = format!("{}:{:0>2}", minutes, seconds);
            painter.text(
                pos2(x + 3.0, rect.top() + 1.0),
                Align2::LEFT_TOP,
                text,
                FontId::new(8., egui::FontFamily::Monospace),
                Color32::from_gray(120),
            );
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.top() + 8.0)],
                Stroke::new(2.0, Color32::from_gray(120)),
            );
            step_index += 1;
            x += step;
        }
    }
}
