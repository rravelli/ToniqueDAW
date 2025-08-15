use std::time::Duration;

use egui::{Rect, Stroke, Vec2};

pub const VIEW_WIDTH: f32 = 1280.;
pub const PIXEL_PER_BEAT: f32 = 10.;
pub const SNAP_DELTA: f32 = 0.2;
pub const MAX_RIGHT: f32 = 50000.;
pub const MIN_LEFT: f32 = -32.;

const XL: f32 = 7000.;
const LG: f32 = 1200.;
const MD: f32 = 200.;
const SM: f32 = 100.;

#[derive(Debug, Clone)]
pub struct WorkspaceGrid {
    pub left: f32,
    pub right: f32,
}

impl WorkspaceGrid {
    pub fn new() -> Self {
        Self {
            left: MIN_LEFT,
            right: 1000.,
        }
    }

    // Converting utils
    pub fn beats_to_x(&self, beats: f32, rect: Rect) -> f32 {
        rect.left() + (beats * PIXEL_PER_BEAT - self.left) * VIEW_WIDTH / (self.right - self.left)
    }

    pub fn duration_to_width(&self, duration: Duration, bpm: f32) -> f32 {
        duration.as_secs_f32() / 60. * bpm * PIXEL_PER_BEAT * VIEW_WIDTH / (self.right - self.left)
    }

    pub fn x_to_beats(&self, x: f32, rect: Rect) -> f32 {
        (((x - rect.left()) * (self.right - self.left) / VIEW_WIDTH + self.left) / PIXEL_PER_BEAT)
            .max(0.)
    }

    pub fn snap_at_grid(&self, beats: f32) -> Option<f32> {
        self.snap_at_grid_with_threshold(beats, SNAP_DELTA)
    }

    pub fn snap_at_grid_with_default(&self, beats: f32) -> f32 {
        if let Some(snapped) = self.snap_at_grid(beats) {
            snapped
        } else {
            beats
        }
    }

    pub fn snap_at_grid_with_threshold_default(&self, beats: f32, threshold: f32) -> f32 {
        if let Some(snapped) = self.snap_at_grid_with_threshold(beats, threshold) {
            snapped
        } else {
            beats
        }
    }

    pub fn snap_at_grid_with_threshold(&self, beats: f32, threshold: f32) -> Option<f32> {
        let delta = self.right - self.left;
        let grid_size = if delta > LG {
            4.
        } else if delta > MD {
            1.
        } else if delta > SM {
            1. / 4.
        } else {
            1. / 8.
        };

        let nearest_position = (beats / grid_size).round() * grid_size;

        if (beats - nearest_position).abs() / grid_size < threshold {
            Some(nearest_position)
        } else {
            None
        }
    }
    /*
     * Zoom the grid centering at specified x position in real coordinates
     */
    pub fn zoom_and_drag_at(&mut self, x: f32, delta: Vec2) {
        let v_mouse_x = self.left + x * (self.right - self.left) / VIEW_WIDTH;

        self.left += (v_mouse_x - self.left) * (delta.y / 100.0);
        self.right -= (self.right - v_mouse_x) * (delta.y / 100.0);

        self.left -= delta.x * (self.right - self.left) / VIEW_WIDTH;
        self.right -= delta.x * (self.right - self.left) / VIEW_WIDTH;

        self.left = self.left.max(MIN_LEFT);
        self.right = self.right.min(MAX_RIGHT);
    }

    pub fn scroll(&mut self, delta: f32) {
        let delta = delta * (self.right - self.left) / VIEW_WIDTH;
        let clipped_delta = (MIN_LEFT - self.left).max(delta);

        self.left += clipped_delta;
        self.right += clipped_delta;
        self.right = self.right.min(MAX_RIGHT);
    }

    pub fn paint(&self, painter: &egui::Painter, rect: egui::Rect) {
        let delta = self.right - self.left;

        let grid_color = egui::Color32::from_gray(50);
        let grid_step = PIXEL_PER_BEAT * VIEW_WIDTH / delta as f32; // 10 pixels per grid line

        // Find the first grid line to draw (leftmost visible)
        let start_x = rect.left() - (self.left * VIEW_WIDTH / delta % grid_step);
        let mut x = start_x;
        let mut beat = (self.left / PIXEL_PER_BEAT) as i32;

        while x < rect.right() {
            let stroke_width = if beat % 4 == 0 {
                2.0 // Thicker line every 4th grid line
            } else {
                1.0 // Normal grid line
            };

            if beat >= 0 && (delta < LG || (delta < XL && beat % 4 == 0) || beat % 16 == 0) {
                painter.line_segment(
                    [
                        egui::Pos2::new(x, rect.top()),
                        egui::Pos2::new(x, rect.bottom()),
                    ],
                    Stroke::new(stroke_width, grid_color),
                );
            }
            x += grid_step;
            beat += 1;
        }

        if delta < MD {
            self.draw_sub_grid(painter, rect);
        }
    }

    fn draw_sub_grid(&self, painter: &egui::Painter, rect: egui::Rect) {
        let delta = self.right - self.left;
        let factor = if delta < 30. {
            32.
        } else if delta < SM {
            8.
        } else {
            4.
        };

        let grid_color = egui::Color32::from_gray(50);
        let grid_step = PIXEL_PER_BEAT * VIEW_WIDTH / (delta as f32 * factor as f32); // 10 pixels per grid line

        // Find the first grid line to draw (leftmost visible)
        let start_x = rect.left() - (self.left as f32 * VIEW_WIDTH / delta as f32 % grid_step);
        let mut x = start_x;

        while x < rect.right() {
            let stroke_width = 0.5;
            painter.line_segment(
                [
                    egui::Pos2::new(x, rect.top()),
                    egui::Pos2::new(x, rect.bottom()),
                ],
                Stroke::new(stroke_width, grid_color),
            );
            x += grid_step;
        }
    }

    pub fn draw_labels(&self, painter: &egui::Painter, rect: egui::Rect, bpm: f32) {
        let delta = self.right - self.left;
        let grid_step = PIXEL_PER_BEAT * VIEW_WIDTH / delta as f32; // 10 pixels per grid line
        let grid_color = egui::Color32::from_gray(90);

        // Find the first grid line to draw (leftmost visible)
        let start_x = rect.left() - (self.left as f32 * VIEW_WIDTH / delta as f32 % grid_step);
        let mut x = start_x;
        let mut beat = (self.left / PIXEL_PER_BEAT) as i32;

        while x < rect.right() {
            if beat >= 0
                && (delta < MD
                    || (delta < 1500. && beat % 4 == 0)
                    || (delta < XL && beat % 16 == 0)
                    || beat % (4 * 16) == 0)
            {
                painter.line_segment(
                    [
                        egui::Pos2::new(x, rect.bottom() - 6.0),
                        egui::Pos2::new(x, rect.bottom() - 2.0),
                    ],
                    Stroke::new(2., grid_color),
                );
                let bar = beat.div_euclid(4);
                let sub_beat = beat % 4;
                // let sub_sub_beat =

                let text = if sub_beat == 0 {
                    format!("{}", bar + 1)
                } else {
                    format!("{}.{}", bar + 1, sub_beat + 1)
                };
                let time = beat as f32 * 60. / bpm;
                let seconds = time.rem_euclid(60.);
                let minutes = time.floor() as usize / 60;

                painter.text(
                    egui::Pos2::new(x, rect.top() + 2.0),
                    egui::Align2::CENTER_TOP,
                    format!("{:0>2}:{:.1}", minutes, seconds),
                    egui::FontId::new(6.0, egui::FontFamily::Monospace),
                    egui::Color32::WHITE,
                );
                painter.text(
                    egui::Pos2::new(x, rect.bottom() - 8.0),
                    egui::Align2::CENTER_BOTTOM,
                    text,
                    egui::FontId::new(8.0, egui::FontFamily::Monospace),
                    egui::Color32::WHITE,
                );
            }
            x += grid_step;
            beat += 1;
        }
    }
}
