use std::sync::RwLockReadGuard;

use egui::{Color32, Rect, Shape, Stroke, pos2};

const MAX_SEGMENT_SIZE: usize = 15;

#[derive(Clone)]
pub struct UIWaveform {}

impl UIWaveform {
    pub fn new() -> Self {
        Self {}
    }

    /// Ultra-fast, streaming-safe waveform painter using subsampling.
    /// Works even if data is still loading (`data.len() < num_samples`).
    pub fn __paint(
        &self,
        shapes: &mut Vec<Shape>,
        rect: Rect,
        data: RwLockReadGuard<'_, (Vec<f32>, Vec<f32>)>,
        start_ratio: f32,
        end_ratio: f32,
        num_samples: u64,
    ) {
        let (left, right) = (&data.0, &data.1);
        let width = rect.width();
        let height = rect.height();

        if width <= 1.0 || num_samples == 0 || start_ratio >= end_ratio {
            return;
        }

        let start_ratio = start_ratio.clamp(0.0, 1.0);
        let end_ratio = end_ratio.clamp(0.0, 1.0);

        // Compute sample window based on full stream (even if not all loaded yet)
        let start_sample = (start_ratio * num_samples as f32) as usize;
        let end_sample = (end_ratio * num_samples as f32) as usize;
        if start_sample >= end_sample {
            return;
        }

        // Bounds of currently loaded data
        let available_len = left.len().min(right.len());
        if available_len == 0 {
            return; // nothing to draw yet
        }

        // Layout
        let half_height = height * 0.5;
        let left_center_y = rect.top() + half_height * 0.5;
        let right_center_y = rect.top() + half_height * 1.5;
        let y_scale = half_height * 0.45;

        // Fixed number of samples drawn (performance cap)
        const MAX_POINTS: usize = 2000;
        let total_samples = end_sample - start_sample;
        let step = (total_samples / MAX_POINTS.max(1)).max(1);

        let mut left_points = Vec::with_capacity(MAX_POINTS);
        let mut right_points = Vec::with_capacity(MAX_POINTS);

        let mut i = start_sample;
        while i < end_sample {
            // Skip samples that aren't loaded yet (draw silence)
            let l = if i < available_len { left[i] } else { 0.0 };
            let r = if i < available_len { right[i] } else { 0.0 };

            let progress = (i - start_sample) as f32 / total_samples as f32;
            let x = rect.left() + progress * width;

            left_points.push(pos2(x, left_center_y - l * y_scale));
            right_points.push(pos2(x, right_center_y - r * y_scale));

            i += step;
        }

        // Draw left + right channels
        shapes.push(Shape::line(left_points, Stroke::new(1.0, Color32::BLACK)));
        shapes.push(Shape::line(right_points, Stroke::new(1.0, Color32::BLACK)));
    }

    pub fn paint(
        &self,
        shapes: &mut Vec<Shape>,
        rect: Rect,
        data: RwLockReadGuard<'_, (Vec<f32>, Vec<f32>)>,
        start_ratio: f32,
        end_ratio: f32,
        num_samples: u64,
        is_stereo: bool,
    ) {
        let mut waveform_shapes = vec![];
        let start_index = (num_samples as f32 * start_ratio).round() as usize;
        let end_index = (num_samples as f32 * end_ratio).round() as usize;
        // safeguard
        if start_index > end_index {
            return;
        }
        let window_len = end_index - start_index;
        let data_len = data.0.len();
        let center_y = rect.center().y;
        let sample_per_pixel = window_len as f32 / rect.width();
        let width = rect.width().max(1.0) as usize;

        for x in 0..width {
            let start_segment = x as f32 * sample_per_pixel;
            let end_segment = (x + 1) as f32 * sample_per_pixel;

            let start = start_segment.floor() as usize + start_index;
            let end = (end_segment.ceil() as usize + start_index).min(data.0.len());

            if start >= data.0.len() {
                break;
            }

            let segment_len = end.max(start + 1).min(data_len) - start;
            let step = (segment_len as f32 / MAX_SEGMENT_SIZE as f32)
                .max(1.)
                .floor() as usize;
            let mut min_left: f32 = 0.;
            let mut max_left: f32 = 0.;
            let mut min_right: f32 = 0.;
            let mut max_right: f32 = 0.;
            let mut i = start;
            while i < end.max(start + 1).min(data_len) {
                let sample = data.0[i];
                min_left = min_left.min(sample);
                max_left = max_left.max(sample);
                if is_stereo && data.1.len() > i {
                    let sample = data.1[i];
                    min_right = min_right.min(sample);
                    max_right = max_right.max(sample);
                }

                i += step;
            }

            let points = if is_stereo {
                [
                    egui::pos2(
                        rect.min.x + x as f32,
                        center_y - rect.height() / 4.0 + min_left * rect.height() / 4.,
                    ),
                    egui::pos2(
                        rect.min.x + x as f32,
                        center_y - rect.height() / 4.0 + max_left * rect.height() / 4.,
                    ),
                ]
            } else {
                [
                    egui::pos2(
                        rect.min.x + x as f32,
                        center_y + min_left * rect.height() / 2.,
                    ),
                    egui::pos2(
                        rect.min.x + x as f32,
                        center_y + max_left * rect.height() / 2.,
                    ),
                ]
            };

            waveform_shapes.push(egui::Shape::line_segment(
                points,
                egui::Stroke::new(1.5, Color32::BLACK),
            ));
            if is_stereo {
                waveform_shapes.push(egui::Shape::line_segment(
                    [
                        egui::pos2(
                            rect.min.x + x as f32,
                            center_y + rect.height() / 4.0 + min_right * rect.height() / 4.,
                        ),
                        egui::pos2(
                            rect.min.x + x as f32,
                            center_y + rect.height() / 4.0 + max_right * rect.height() / 4.,
                        ),
                    ],
                    egui::Stroke::new(1.5, Color32::BLACK),
                ));
            }
        }
        shapes.extend(waveform_shapes);
    }
}
