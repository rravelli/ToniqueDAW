use egui::{Color32, Painter, Pos2, Rect, Vec2};

use crate::metrics::AudioMetrics;

const RANGE: f32 = 60.;
const SPACE: f32 = 1.;
#[derive(Clone)]
pub struct LoudnessMeter {}

impl LoudnessMeter {
    pub fn paint(&self, painter: &Painter, rect: Rect, metrics: AudioMetrics, gray: bool) {
        let peak = metrics.get_peak();
        let rms = metrics.get_rms();

        let color1 = if gray {
            Color32::from_gray(40)
        } else if peak[0] > 1. {
            Color32::from_rgb(255, 100, 100)
        } else {
            Color32::ORANGE
        };
        let color2 = if gray {
            Color32::from_gray(70)
        } else if peak[0] > 1. {
            Color32::from_rgb(255, 3, 10)
        } else {
            Color32::GREEN
        };

        self.paint_rect(&[1., 1.], painter, Color32::from_gray(0), rect);
        self.paint_rect(&peak, painter, color1, rect);
        self.paint_rect(&rms, painter, color2, rect);
    }

    fn paint_rect(&self, values: &[f32; 2], painter: &Painter, color: Color32, rect: Rect) {
        let rect_width = (rect.width() - SPACE) / 2.;
        let left_height = to_scale(values[0]) * rect.height();
        let right_height = to_scale(values[1]) * rect.height();

        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(rect.min.x, rect.bottom() - left_height),
                Vec2::new(rect_width, left_height),
            ),
            0.,
            color,
        );
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(
                    rect.min.x + rect_width + SPACE,
                    rect.bottom() - right_height,
                ),
                Vec2::new(rect_width, right_height),
            ),
            0.,
            color,
        );
    }
}

fn to_scale(value: f32) -> f32 {
    ((20. * value.log10() + RANGE) / RANGE).clamp(0., 1.)
}
