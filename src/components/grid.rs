use std::time::Duration;

use egui::{Rect, Stroke, Vec2};

pub const VIEW_WIDTH: f32 = 1280.;
pub const PIXEL_PER_BEAT: f32 = 10.;
pub const SNAP_DELTA: f32 = 0.2;
pub const MAX_RIGHT: f32 = 50000.;
pub const MIN_LEFT: f32 = -1.;

const LG: f32 = 1200.;
const MD: f32 = 200.;

#[derive(Debug, Clone)]
pub struct WorkspaceGrid {
    pub left: f32,
    pub right: f32,
}

impl WorkspaceGrid {
    pub fn new() -> Self {
        Self {
            left: 0.,
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

    pub fn snap_at_grid(&self, beats: f32) -> f32 {
        let delta = self.right - self.left;
        let grid_size = if delta > LG {
            4.
        } else if delta > MD {
            1.
        } else {
            1. / 4.
        };

        let nearest_position = (beats / grid_size).round() * grid_size;

        if (beats - nearest_position).abs() / grid_size < SNAP_DELTA {
            nearest_position
        } else {
            beats
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
        let mut beat = (self.left / PIXEL_PER_BEAT) as usize;

        while x < rect.right() {
            let stroke_width = if beat % 4 == 0 {
                2.0 // Thicker line every 4th grid line
            } else {
                1.0 // Normal grid line
            };

            if delta < LG || beat % 4 == 0 {
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
        } else if delta < 40. {
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
}
