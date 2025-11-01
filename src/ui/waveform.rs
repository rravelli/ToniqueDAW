use std::sync::RwLockReadGuard;

use egui::{Color32, Rect, Shape};

const MAX_SEGMENT_SIZE: usize = 15;

#[derive(Clone)]
pub struct UIWaveform {}

impl UIWaveform {
    pub fn new() -> Self {
        Self {}
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
        color: Color32,
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
        let stroke = egui::Stroke::new(1.5, color);
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

            waveform_shapes.push(egui::Shape::line_segment(points, stroke));
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
                    stroke,
                ));
            }
        }
        shapes.extend(waveform_shapes);
    }
}
