//! Editor of [`EffectKind::Saturation`](crate::core::effect::EffectKind::Saturation):
//! its curve with the drive, output and mix applied, the curve's buttons,
//! and drive, output and mix knobs.

use crate::ui::theme::font_size;
use crate::{
    core::effect::Setting,
    ui::{
        effects::{EditorContext, EffectEdit, EffectEditor, format_db, format_percent, graph_rect},
        theme::ThemeExt,
        widget::flat_button::FlatButton,
    },
};
use egui::{Align, FontId, Layout, Pos2, Sense, Stroke, Ui, pos2, vec2};
use tonique_engine::{dsp::db_to_gain, edit::PluginKind, nodes::SaturationCurve};

/// Output levels shown, either way.
const RANGE: f32 = 1.5;
const BUTTON_WIDTH: f32 = 40.;
const BUTTON_HEIGHT: f32 = 16.;

pub struct SaturationEditor;

fn curve_name(curve: SaturationCurve) -> (&'static str, &'static str) {
    match curve {
        SaturationCurve::SoftClip => ("Soft", "Soft clip: rounds peaks off smoothly"),
        SaturationCurve::Tape => ("Tape", "Tape: soft and uneven, adding even harmonics"),
        SaturationCurve::HardClip => ("Hard", "Hard clip: cuts peaks flat"),
    }
}

impl EffectEditor for SaturationEditor {
    fn ui(&mut self, ui: &mut Ui, cx: &mut EditorContext) {
        let PluginKind::Saturation(curve) = cx.effect.plugin.kind else {
            return;
        };
        let value = |name| cx.param(name).map_or(0., |p| p.get());
        let (drive, output, mix) = (
            db_to_gain(value("drive")),
            db_to_gain(value("output")),
            value("mix"),
        );

        let (rect, _) = graph_rect(ui, Sense::hover());
        let theme = ui.app_theme();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2., theme.bg_deep);
        // Input across, output up, both from -1 to 1 at the sides.
        let half = rect.height() / 2.;
        let at = |x: f32, y: f32| {
            pos2(
                rect.center().x + x * half / RANGE * (rect.width() / rect.height()).min(1.5),
                rect.center().y - (y / RANGE).clamp(-1., 1.) * half,
            )
        };
        for level in [-1., 1.] {
            painter.hline(
                rect.x_range(),
                at(0., level).y,
                Stroke::new(1., theme.grid_beat),
            );
            painter.vline(
                at(level, 0.).x,
                rect.y_range(),
                Stroke::new(1., theme.grid_beat),
            );
        }
        painter.hline(
            rect.x_range(),
            rect.center().y,
            Stroke::new(1., theme.grid_bar),
        );
        painter.vline(
            rect.center().x,
            rect.y_range(),
            Stroke::new(1., theme.grid_bar),
        );
        painter.line_segment(
            [at(-RANGE, -RANGE), at(RANGE, RANGE)],
            Stroke::new(1., theme.grid_bar),
        );
        let color = if cx.enabled() {
            theme.accent
        } else {
            theme.text_disabled
        };
        let steps = rect.width().ceil() as usize;
        let points: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let x = -RANGE + 2. * RANGE * i as f32 / steps as f32;
                let wet = curve.shape(x * drive) * output;
                at(x, x * (1. - mix) + wet * mix)
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.5, color)));

        ui.horizontal(|ui| {
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 2.;
                for option in SaturationCurve::ALL {
                    let (name, tooltip) = curve_name(option);
                    let button = FlatButton::new(name)
                        .size(vec2(BUTTON_WIDTH, BUTTON_HEIGHT))
                        .font(FontId::proportional(font_size::SMALL))
                        .selected(curve == option)
                        .tooltip(tooltip);
                    if ui.add(button).clicked() && curve != option {
                        cx.edits.push(EffectEdit::Setting(Setting::Curve(option)));
                    }
                }
            });
            cx.param_knob(ui, "drive", "Drive", &format_db, false);
            cx.param_knob(ui, "output", "Output", &format_db, false);
            cx.param_knob(ui, "mix", "Mix", &format_percent, false);
        });
    }

    fn width(&self) -> f32 {
        220.
    }
}
