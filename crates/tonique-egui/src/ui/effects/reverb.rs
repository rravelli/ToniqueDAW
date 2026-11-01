//! Editor of [`EffectKind::Reverb`](crate::core::effect::EffectKind::Reverb):
//! how its tail fades after the pre-delay, highs sooner as it's damped, and
//! size, decay, damping, pre-delay and mix knobs.

use crate::ui::theme::font_size;
use crate::ui::{
    effects::{EditorContext, EffectEditor, dynamics::format_time_ms, format_percent, graph_rect},
    theme::ThemeExt,
};
use egui::{Align2, FontId, Pos2, Sense, Shape, Stroke, Ui, pos2, vec2};

/// Levels shown, in dB below the start of the tail.
const FLOOR_DB: f32 = 60.;
/// How much sooner the highs fade when fully damped.
const DAMPED_HIGHS: f32 = 0.8;

pub struct ReverbEditor;

pub fn format_seconds(seconds: f32) -> String {
    format!("{seconds:.1} s")
}

impl EffectEditor for ReverbEditor {
    fn ui(&mut self, ui: &mut Ui, cx: &mut EditorContext) {
        let value = |name| cx.param(name).map_or(0., |p| p.get());
        let (decay, damping, predelay) =
            (value("decay"), value("damping"), value("predelay") / 1000.);

        let (rect, _) = graph_rect(ui, Sense::hover());
        let theme = ui.app_theme();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2., theme.bg_deep);
        let inner = rect.shrink2(vec2(4., 4.));
        let span = (predelay + decay) * 1.15;
        let x = |t: f32| inner.left() + t / span * inner.width();
        let y = |db: f32| inner.top() + (db / FLOOR_DB).clamp(0., 1.) * inner.height();
        for db in [20., 40.] {
            painter.hline(rect.x_range(), y(db), Stroke::new(1., theme.grid_beat));
        }
        let color = if cx.enabled() {
            theme.accent
        } else {
            theme.text_disabled
        };
        // Falls 60 dB over the decay, in a straight line in dB.
        let tail = |decay: f32| -> Vec<Pos2> {
            vec![
                pos2(x(0.), inner.bottom()),
                pos2(x(predelay), inner.bottom()),
                pos2(x(predelay), y(0.)),
                pos2(x(predelay + decay), y(FLOOR_DB)),
            ]
        };
        let highs = decay * (1. - damping.clamp(0., 1.) * DAMPED_HIGHS);
        painter.add(Shape::convex_polygon(
            vec![
                pos2(x(predelay), inner.bottom()),
                pos2(x(predelay), y(0.)),
                pos2(x(predelay + decay), y(FLOOR_DB)),
            ],
            color.gamma_multiply(0.15),
            Stroke::NONE,
        ));
        painter.add(Shape::line(tail(highs), Stroke::new(1., theme.text_muted)));
        painter.add(Shape::line(tail(decay), Stroke::new(1.5, color)));
        painter.text(
            rect.right_top() + vec2(-4., 3.),
            Align2::RIGHT_TOP,
            format!("{} to -60 dB", format_seconds(decay)),
            FontId::proportional(font_size::SMALL),
            theme.text_muted,
        );

        ui.horizontal(|ui| {
            cx.param_knob(ui, "size", "Size", &format_percent, false);
            cx.param_knob(ui, "decay", "Decay", &format_seconds, true);
            cx.param_knob(ui, "damping", "Damping", &format_percent, false);
            cx.param_knob(ui, "predelay", "Pre-delay", &format_time_ms, false);
            cx.param_knob(ui, "mix", "Mix", &format_percent, false);
        });
    }

    fn width(&self) -> f32 {
        272.
    }
}
