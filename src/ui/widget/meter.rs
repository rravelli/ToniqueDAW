use crate::core::metrics::AudioMetrics;
use egui::{Color32, Painter, Pos2, Rect, Sense, Vec2, Widget};

const RANGE: f32 = 60.;
const SPACE: f32 = 1.;
#[derive(Clone)]
pub struct LoudnessMeter {
    size: Vec2,
    disabled: bool,
    metrics: AudioMetrics,
}

impl LoudnessMeter {
    pub fn new(size: Vec2, metrics: AudioMetrics) -> Self {
        Self {
            size,
            disabled: false,
            metrics,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
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

impl Widget for LoudnessMeter {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (res, painter) = ui.allocate_painter(self.size, Sense::click());
        let rect = res.rect;
        let peak = self.metrics.get_peak();
        let rms = self.metrics.get_rms();

        let color1 = if self.disabled {
            Color32::from_gray(40)
        } else if peak[0] > 1. {
            Color32::from_rgb(255, 100, 100)
        } else {
            Color32::ORANGE
        };
        let color2 = if self.disabled {
            Color32::from_gray(70)
        } else if peak[0] > 1. {
            Color32::from_rgb(255, 3, 10)
        } else {
            Color32::GREEN
        };

        self.paint_rect(&[1., 1.], &painter, Color32::from_gray(0), rect);
        self.paint_rect(&peak, &painter, color1, rect);
        self.paint_rect(&rms, &painter, color2, rect);
        res
    }
}
