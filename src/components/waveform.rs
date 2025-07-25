use std::sync::MutexGuard;

use egui::{Color32, Painter, Rect};

const MAX_SEGMENT_SIZE: usize = 150;

#[derive(Clone)]
pub struct UIWaveform {}

impl UIWaveform {
    pub fn new() -> Self {
        Self {}
    }

    pub fn paint(
        &self,
        painter: &Painter,
        rect: Rect,
        data: MutexGuard<'_, (Vec<f32>, Vec<f32>)>,
        start_ratio: f32,
        end_ratio: f32,
        num_samples: u64,
    ) {
        let mut shapes = vec![];

        let start_index = (num_samples as f32 * start_ratio).floor() as usize;
        let end_index = (num_samples as f32 * end_ratio).ceil() as usize;
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
            let mut min: f32 = 0.;
            let mut max: f32 = 0.;
            let mut i = start;
            while i < end.max(start + 1).min(data_len) {
                let sample = data.0[i];
                min = min.min(sample);
                max = max.max(sample);
                i += step;
            }

            shapes.push(egui::Shape::line_segment(
                [
                    egui::pos2(rect.min.x + x as f32, center_y + min * rect.height() / 2.),
                    egui::pos2(rect.min.x + x as f32, center_y + max * rect.height() / 2.),
                ],
                egui::Stroke::new(1., Color32::BLACK),
            ));
        }

        painter.add(shapes);
    }
}
