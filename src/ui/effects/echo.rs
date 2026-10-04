//! Editor of [`EffectKind::Echo`](crate::core::effect::EffectKind::Echo):
//! its repeats fading out, and time, feedback and mix knobs.

use crate::{
    core::effect::{ECHO_TIME, ECHO_TIMES, Setting},
    ui::{
        effects::{EditorContext, EffectEdit, EffectEditor, format_ms, format_percent, graph_rect},
        theme::ThemeExt,
    },
};
use egui::{Align2, FontId, Sense, Stroke, Ui, pos2};
use tonique_engine::edit::PluginKind;

/// Repeats drawn, at most.
const REPEATS: usize = 12;

pub struct EchoEditor;

impl EffectEditor for EchoEditor {
    fn ui(&mut self, ui: &mut Ui, cx: &mut EditorContext) {
        let PluginKind::Echo { time_s } = cx.effect.plugin.kind else {
            return;
        };
        let feedback = cx.param("feedback").map_or(0., |p| p.get());
        let mix = cx.param("mix").map_or(0., |p| p.get());

        let (rect, _) = graph_rect(ui, Sense::hover());
        let theme = ui.app_theme();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2., theme.bg_deep);
        let color = if cx.enabled() {
            theme.accent
        } else {
            theme.text_disabled
        };
        // The dry signal, then each repeat a delay later, quieter by the
        // feedback.
        let inner = rect.shrink2(egui::vec2(8., 6.));
        let step = inner.width() / REPEATS as f32;
        let mut level = mix;
        for i in 0..=REPEATS {
            let (height, color) = if i == 0 {
                (1., theme.text_muted)
            } else {
                let height = level;
                level *= feedback;
                (height, color)
            };
            if height < 0.01 {
                break;
            }
            let x = inner.left() + i as f32 * step;
            painter.line_segment(
                [
                    pos2(x, inner.bottom()),
                    pos2(x, inner.bottom() - height * inner.height()),
                ],
                Stroke::new(3., color),
            );
        }
        painter.text(
            rect.right_top() + egui::vec2(-4., 3.),
            Align2::RIGHT_TOP,
            format!("{} per repeat", format_ms(time_s)),
            FontId::proportional(9.),
            theme.text_muted,
        );

        ui.horizontal(|ui| {
            if let Some(time) =
                cx.setting_knob(ui, time_s, ECHO_TIMES, ECHO_TIME, "Time", &format_ms, true)
            {
                cx.edits.push(EffectEdit::Setting(Setting::Time(time)));
            }
            cx.param_knob(ui, "feedback", "Feedback", &format_percent, false);
            cx.param_knob(ui, "mix", "Mix", &format_percent, false);
        });
    }

    fn width(&self) -> f32 {
        200.
    }
}
