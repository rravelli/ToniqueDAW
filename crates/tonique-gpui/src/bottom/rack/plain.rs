//! Editors of the compressor, limiter, gate, saturation and reverb: their
//! knobs (and the saturation's curve), as the egui app's, without its
//! graphs yet.

use gpui::{AnyElement, Context, div, prelude::*, px};
use tonique_engine::{
    edit::PluginKind,
    nodes::{GATE_SILENT_DB, SaturationCurve},
};

use crate::{
    app::Tonique,
    bottom::rack::{
        editors::{EditorInput, KNOB_GAP},
        format_db, format_percent, format_ratio, format_seconds, format_time_ms,
        knob::{KNOB_HEIGHT, SettingKnob},
    },
    core::effect::{LIMITER_LOOKAHEAD, LIMITER_LOOKAHEADS, Setting},
    theme::{Theme, size},
    widgets::Flat,
};

/// Width of the saturation's curve buttons.
pub const CURVE_WIDTH: f32 = 40.;
const CURVE_HEIGHT: f32 = 16.;

/// A knob for a parameter: its name, label, format and whether it's on a
/// log scale.
type KnobSpec = (&'static str, &'static str, fn(f32) -> String, bool);

fn format_range(db: f32) -> String {
    if db <= GATE_SILENT_DB {
        "-inf dB".into()
    } else {
        format!("{db:.0} dB")
    }
}

fn curve_name(curve: SaturationCurve) -> &'static str {
    match curve {
        SaturationCurve::SoftClip => "Soft",
        SaturationCurve::Tape => "Tape",
        SaturationCurve::HardClip => "Hard",
    }
}

impl Tonique {
    /// A row of `knobs` for `input`'s parameters, then `more`.
    fn knob_row(
        &self,
        input: &EditorInput,
        knobs: &[KnobSpec],
        more: impl IntoIterator<Item = AnyElement>,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let effect = input.effect;
        div()
            .flex()
            .flex_none()
            .gap(px(KNOB_GAP))
            .h(px(KNOB_HEIGHT))
            .children(knobs.iter().filter_map(|&(param, name, format, log)| {
                self.param_knob(effect, param, name, format, log, theme, context)
            }))
            .children(more)
    }

    /// Centred in the frame's body.
    fn centred(row: gpui::Div) -> AnyElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(row)
            .into_any_element()
    }

    pub(super) fn compressor_body(
        &self,
        input: &EditorInput,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let row = self.knob_row(
            input,
            &[
                ("attack", "Attack", format_time_ms, true),
                ("release", "Release", format_time_ms, true),
                ("ratio", "Ratio", format_ratio, true),
                ("threshold", "Threshold", format_db, false),
                ("knee", "Knee", format_db, false),
                ("makeup", "Makeup", format_db, false),
            ],
            [],
            theme,
            context,
        );
        Self::centred(row)
    }

    pub(super) fn limiter_body(
        &self,
        input: &EditorInput,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let PluginKind::Limiter { lookahead_ms } = input.effect.plugin.kind else {
            return div().into_any_element();
        };
        let lookahead = self.setting_knob(
            input,
            SettingKnob::Lookahead,
            (lookahead_ms, LIMITER_LOOKAHEADS, LIMITER_LOOKAHEAD),
            "Ahead",
            format_time_ms,
            true,
            theme,
            context,
        );
        let row = self.knob_row(
            input,
            &[
                ("gain", "Gain", format_db, false),
                ("ceiling", "Ceiling", format_db, false),
                ("release", "Release", format_time_ms, true),
            ],
            [lookahead.into_any_element()],
            theme,
            context,
        );
        Self::centred(row)
    }

    pub(super) fn gate_body(
        &self,
        input: &EditorInput,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let row = self.knob_row(
            input,
            &[
                ("threshold", "Threshold", format_db, false),
                ("range", "Range", format_range, false),
                ("hold", "Hold", format_time_ms, false),
                ("attack", "Attack", format_time_ms, true),
                ("release", "Release", format_time_ms, true),
            ],
            [],
            theme,
            context,
        );
        Self::centred(row)
    }

    /// Its curve picked on buttons, and drive, output and mix knobs.
    pub(super) fn saturation_body(
        &self,
        input: &EditorInput,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let PluginKind::Saturation(curve) = input.effect.plugin.kind else {
            return div().into_any_element();
        };
        let (track, plugin) = (input.track, input.effect.plugin.id);
        let curves = SaturationCurve::ALL.map(|option| {
            let name = curve_name(option);
            Flat::new(name, CURVE_HEIGHT)
                .width(CURVE_WIDTH)
                .selected(curve == option)
                .show(
                    div().text_size(px(size::SMALL)).child(name),
                    theme,
                    context,
                    move |this, _| {
                        if curve != option {
                            this.state
                                .set_effect_setting(&track, plugin, Setting::Curve(option));
                        }
                    },
                )
        });
        let picker = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(curves)
            .into_any_element();
        let knobs = self.knob_row(
            input,
            &[
                ("drive", "Drive", format_db, false),
                ("output", "Output", format_db, false),
                ("mix", "Mix", format_percent, false),
            ],
            [],
            theme,
            context,
        );
        let row = div()
            .flex()
            .items_center()
            .gap(px(KNOB_GAP))
            .child(picker)
            .child(knobs);
        Self::centred(row)
    }

    pub(super) fn reverb_body(
        &self,
        input: &EditorInput,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let row = self.knob_row(
            input,
            &[
                ("size", "Size", format_percent, false),
                ("decay", "Decay", format_seconds, true),
                ("damping", "Damping", format_percent, false),
                ("predelay", "Pre-delay", format_time_ms, false),
                ("mix", "Mix", format_percent, false),
            ],
            [],
            theme,
            context,
        );
        Self::centred(row)
    }
}
