//! Editor of [`EffectKind::Echo`](crate::core::effect::EffectKind::Echo):
//! its repeats fading out, a time knob (or a note value when synced to the
//! tempo), and feedback, mix and feedback cut knobs.

use crate::ui::theme::font_size;
use crate::{
    core::effect::{ECHO_SYNC, ECHO_SYNCS, ECHO_TIME, ECHO_TIMES, Setting},
    ui::{
        effects::{
            EditorContext, EffectEdit, EffectEditor, TOGGLE_SIZE, format_hz, format_ms,
            format_percent, graph_rect,
        },
        theme::ThemeExt,
        widget::flat_button::FlatButton,
    },
};
use egui::{Align, Align2, FontId, Layout, Sense, Stroke, Ui, pos2};
use tonique_engine::edit::PluginKind;

/// Repeats drawn, at most.
const REPEATS: usize = 12;

pub struct EchoEditor;

/// The place in [`ECHO_SYNCS`] of the note value nearest `beats`.
pub fn nearest_sync(beats: f32) -> usize {
    ECHO_SYNCS
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (a.0 - beats).abs().total_cmp(&(b.0 - beats).abs()))
        .map_or(0, |(i, _)| i)
}

impl EffectEditor for EchoEditor {
    fn ui(&mut self, ui: &mut Ui, cx: &mut EditorContext) {
        let PluginKind::Echo { time_s, sync } = cx.effect.plugin.kind else {
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
        let per_repeat = match sync {
            Some(beats) => ECHO_SYNCS[nearest_sync(beats)].1.to_string(),
            None => format_ms(time_s),
        };
        painter.text(
            rect.right_top() + egui::vec2(-4., 3.),
            Align2::RIGHT_TOP,
            format!("{per_repeat} per repeat"),
            FontId::proportional(font_size::SMALL),
            theme.text_muted,
        );

        ui.horizontal(|ui| {
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                let button = FlatButton::new("Sync")
                    .size(TOGGLE_SIZE)
                    .font(FontId::proportional(font_size::SMALL))
                    .selected(sync.is_some())
                    .tooltip("Repeat on note values, following the tempo");
                if ui.add(button).clicked() {
                    let sync = match sync {
                        Some(_) => None,
                        None => Some(ECHO_SYNC),
                    };
                    cx.edits.push(EffectEdit::Setting(Setting::Sync(sync)));
                }
            });
            match sync {
                Some(beats) => {
                    let last = (ECHO_SYNCS.len() - 1) as f32;
                    let default = nearest_sync(ECHO_SYNC) as f32;
                    let name =
                        |i: f32| ECHO_SYNCS[(i.round() as usize).min(ECHO_SYNCS.len() - 1)].1;
                    if let Some(i) = cx.setting_knob(
                        ui,
                        nearest_sync(beats) as f32,
                        0.0..=last,
                        default,
                        "Time",
                        &|i| name(i).to_string(),
                        false,
                    ) {
                        let beats = ECHO_SYNCS[(i.round() as usize).min(ECHO_SYNCS.len() - 1)].0;
                        cx.edits
                            .push(EffectEdit::Setting(Setting::Sync(Some(beats))));
                    }
                }
                None => {
                    if let Some(time) =
                        cx.setting_knob(ui, time_s, ECHO_TIMES, ECHO_TIME, "Time", &format_ms, true)
                    {
                        cx.edits.push(EffectEdit::Setting(Setting::Time(time)));
                    }
                }
            }
            cx.param_knob(ui, "feedback", "Feedback", &format_percent, false);
            cx.param_knob(ui, "mix", "Mix", &format_percent, false);
            cx.param_knob(ui, "low_cut", "Low cut", &format_hz, true);
            cx.param_knob(ui, "high_cut", "High cut", &format_hz, true);
        });
    }

    fn width(&self) -> f32 {
        316.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syncs_snap_to_the_nearest_note_value() {
        assert_eq!(ECHO_SYNCS[nearest_sync(0.5)].1, "1/8");
        assert_eq!(ECHO_SYNCS[nearest_sync(0.7)].1, "1/4T");
        assert_eq!(ECHO_SYNCS[nearest_sync(100.)].1, "1/1");
    }
}
