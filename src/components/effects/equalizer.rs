use std::f32::consts::PI;

use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use fundsp::{
    hacker::{AudioUnit, shared},
    hacker32::{lowpass, pass, var},
    shared::Shared,
};
use rtrb::Producer;

use crate::{
    components::{buttons::paint_circle_button, effect::UIEffectContent},
    message::GuiToPlayerMsg,
    metrics::AudioMetrics,
};

const BOTTOM_HEIGHT: f32 = 50.;

#[derive(Clone)]
pub struct EqualizerEffect {
    id: String,
    q: f32,
    cutoff: f32,

    cutoff_shared: Shared,
    q_shared: Shared,

    min_freq: f32,
    max_freq: f32,
}

impl EqualizerEffect {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4().into(),
            cutoff: 1300.,
            q: 0.5,
            cutoff_shared: shared(1300.),
            q_shared: shared(0.5),

            min_freq: 50.,
            max_freq: 20_000.,
        }
    }

    fn format_freq(f: f32) -> String {
        if f < 1000. {
            return format!("{:.0}Hz", f);
        } else {
            return format!("{:.0}kHz", f / 1000.);
        }
    }

    fn paint_grid(&self, shapes: &mut Vec<Shape>, rect: Rect) {
        let mut f = self.min_freq;
        let mut mul = 10.;
        let mut pow = 2.;
        while f < self.max_freq {
            f += mul;
            if f.log10() >= pow {
                mul *= 10.;
                pow += 1.
            }
            let x = rect.left()
                + ((f.log10() - self.min_freq.log10())
                    / (self.max_freq.log10() - self.min_freq.log10()))
                    * rect.width();
            shapes.push(Shape::line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                Stroke::new(1.0, Color32::from_gray(40)),
            ));
        }
    }

    fn paint_spectrum(&self, shapes: &mut Vec<Shape>, rect: Rect, metrics: &mut AudioMetrics) {
        let spectrum = metrics.get_fft();
        let n = spectrum.len();
        let sample_rate = 44100.;
        let bin_freqs: Vec<f32> = (0..n).map(|i| i as f32 * sample_rate / n as f32).collect();
        let mut max = -100.;
        let mut prev = None;

        for i in 0..n {
            let x = rect.left()
                + ((bin_freqs[i].log10() - self.min_freq.log10())
                    / (self.max_freq.log10() - self.min_freq.log10()))
                    * rect.width();
            let y = rect.bottom() - ((spectrum[i] + 14.0) / 28.0).clamp(0.0, 1.0) * rect.height();
            let pos = Pos2::new(x, y);
            if spectrum[i] > max {
                max = spectrum[i];
            }
            if let Some(prev) = prev {
                shapes.push(Shape::line_segment(
                    [prev, pos],
                    Stroke::new(1.0, Color32::from_gray(80)),
                ));
            }
            prev = Some(pos);
        }
    }
}

impl UIEffectContent for EqualizerEffect {
    fn ui(
        &mut self,
        ui: &mut Ui,
        metrics: &mut AudioMetrics,
        enabled: bool,
        _: &mut Producer<GuiToPlayerMsg>,
    ) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::all());
        let full_rect = response.rect;

        let rect = Rect::from_min_size(
            full_rect.min,
            Vec2::new(full_rect.width(), full_rect.height() - BOTTOM_HEIGHT),
        );

        let mut shapes = Vec::new();
        let mut last_pos = None;
        // // Filter parameters
        let sample_rate = 44100.0;
        // // Compute biquad coefficients (digital 2nd order lowpass)
        let omega_c = 2.0 * PI * self.cutoff / sample_rate;
        let alpha = omega_c.sin() / (2.0 * self.q);
        let cos_omega_c = omega_c.cos();

        let b0 = (1.0 - cos_omega_c) / 2.0;
        let b1 = 1.0 - cos_omega_c;
        let b2 = (1.0 - cos_omega_c) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_omega_c;
        let a2 = 1.0 - alpha;

        // Frequency range (logarithmic)

        let n_points = 500;

        self.paint_grid(&mut shapes, rect);
        if enabled {
            self.paint_spectrum(&mut shapes, rect, metrics);
        }
        for i in 0..n_points {
            let freq = self.min_freq
                * (self.max_freq / self.min_freq).powf(i as f32 / (n_points - 1) as f32);

            let omega = 2.0 * PI * freq / sample_rate;
            let cos_omega = omega.cos();
            let sin_omega = omega.sin();

            // Calculate numerator and denominator (complex)
            let num_re = b0 + b1 * cos_omega + b2 * cos_omega * cos_omega;
            let num_im = b1 * sin_omega + b2 * 2.0 * cos_omega * sin_omega;
            let den_re = a0 + a1 * cos_omega + a2 * cos_omega * cos_omega;
            let den_im = a1 * sin_omega + a2 * 2.0 * cos_omega * sin_omega;

            let num_mag = (num_re * num_re + num_im * num_im).sqrt();
            let den_mag = (den_re * den_re + den_im * den_im).sqrt();

            let mag = num_mag / den_mag;
            let db = 20.0 * mag.log10();

            // X: log frequency
            let x = rect.left()
                + ((freq.log10() - self.min_freq.log10())
                    / (self.max_freq.log10() - self.min_freq.log10()))
                    * rect.width();
            // Y: dB scale (from +6 to -60 dB)
            let y = rect.bottom() - ((db + 14.0) / 28.0).clamp(0.0, 1.0) * rect.height();

            if let Some(last) = last_pos {
                shapes.push(Shape::line_segment(
                    [last, egui::pos2(x, y)],
                    egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
                ));
            }
            last_pos = Some(egui::pos2(x, y));
        }
        shapes.push(Shape::line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(1.0, Color32::DARK_GRAY),
        ));

        let label = Self::format_freq(self.cutoff);

        let freq_res = paint_circle_button(
            ui,
            &painter,
            Pos2::new(rect.left() + 20., rect.bottom() + BOTTOM_HEIGHT / 2.),
            &mut self.cutoff,
            self.id.clone().into(),
            "Freq".into(),
            label.into(),
            self.min_freq,
            self.max_freq,
            true,
        );

        let label = format!("{:.1}", self.q.clone());

        let q_res = paint_circle_button(
            ui,
            &painter,
            Pos2::new(rect.left() + 50., rect.bottom() + BOTTOM_HEIGHT / 2.),
            &mut self.q,
            self.id.clone().into(),
            "Q".into(),
            Some(label),
            0.5,
            10.0,
            false,
        );

        if q_res.dragged() || freq_res.dragged() {
            self.q_shared.set_value(self.q);
            self.cutoff_shared.set_value(self.cutoff);
        }

        painter.add(shapes);
    }

    fn width(&self) -> f32 {
        300.
    }

    fn get_unit(&self) -> Box<dyn AudioUnit> {
        let filter = (pass() | var(&self.cutoff_shared) | var(&self.q_shared)) >> lowpass();
        Box::new(filter.clone() | filter.clone())
    }

    fn id(&self) -> String {
        self.id.clone()
    }
}
