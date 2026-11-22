//! Editors of the saturation and the reverb, as the egui app's: the
//! saturation's curve, output against input, with its curve picked on
//! buttons; how the reverb's tail fades after the pre-delay, highs sooner
//! as it's damped. Then their knobs.

use gpui::{AnyElement, Context, canvas, div, prelude::*, px};
use tonique_engine::{dsp::db_to_gain, edit::PluginKind, nodes::SaturationCurve};

use crate::{
    app::Tonique,
    bottom::{
        paint::{Align, fill, fill_rounded, polygon, polyline, text},
        rack::{
            editors::{EditorInput, KNOB_GAP},
            format_db, format_percent, format_seconds, format_time_ms,
            graph::hline,
        },
    },
    core::effect::Setting,
    geometry::Rect,
    theme::{Theme, faded, size},
    widgets::Flat,
};

/// Width of the saturation's curve buttons.
pub const CURVE_WIDTH: f32 = 40.;
const CURVE_HEIGHT: f32 = 16.;
/// Input levels the saturation's graph spans each side of 0.
const RANGE: f32 = 1.5;
/// Levels a reverb's graph shows, in dB below the start of its tail.
const FLOOR_DB: f32 = 60.;
/// How much sooner the highs fade when fully damped.
const DAMPED_HIGHS: f32 = 0.8;

fn curve_name(curve: SaturationCurve) -> &'static str {
    match curve {
        SaturationCurve::SoftClip => "Soft",
        SaturationCurve::Tape => "Tape",
        SaturationCurve::HardClip => "Hard",
    }
}

impl Tonique {
    /// Its curve, its curve's buttons, and drive, output and mix knobs.
    pub(super) fn saturation_body(
        &self,
        input: &EditorInput,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let effect = input.effect;
        let PluginKind::Saturation(curve) = effect.plugin.kind else {
            return div().into_any_element();
        };
        let value = |name| effect.plugin.param(name).map_or(0., |param| param.get());
        let (drive, output, mix) = (
            db_to_gain(value("drive")),
            db_to_gain(value("output")),
            value("mix"),
        );
        let enabled = input.enabled();
        let paint_theme = theme.clone();
        let graph = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let theme = &paint_theme;
                let rect = Rect::of(bounds);
                fill_rounded(window, rect, 2., theme.bg_deep);
                // Input across, output up, both from -1 to 1 at the sides.
                let half = rect.height / 2.;
                let centre = (rect.left + rect.width / 2., rect.top + half);
                let across = half / RANGE * (rect.width / rect.height).min(1.5);
                let at = |x: f32, y: f32| {
                    (
                        centre.0 + x * across,
                        centre.1 - (y / RANGE).clamp(-1., 1.) * half,
                    )
                };
                for level in [-1., 1.] {
                    hline(window, rect, at(0., level).1, theme.grid_beat);
                    let x = at(level, 0.).0;
                    fill(
                        window,
                        Rect::new(x.round(), rect.top, 1., rect.height),
                        theme.grid_beat,
                    );
                }
                hline(window, rect, centre.1, theme.grid_bar);
                fill(
                    window,
                    Rect::new(centre.0.round(), rect.top, 1., rect.height),
                    theme.grid_bar,
                );
                polyline(
                    window,
                    &[at(-RANGE, -RANGE), at(RANGE, RANGE)],
                    1.,
                    theme.grid_bar,
                );
                let color = if enabled {
                    theme.accent
                } else {
                    theme.text_disabled
                };
                let steps = rect.width.ceil() as usize;
                let points: Vec<(f32, f32)> = (0..=steps)
                    .map(|step| {
                        let x = -RANGE + 2. * RANGE * step as f32 / steps as f32;
                        let wet = curve.shape(x * drive) * output;
                        at(x, x * (1. - mix) + wet * mix)
                    })
                    .filter(|(x, _)| *x >= rect.left && *x <= rect.right())
                    .collect();
                polyline(window, &points, 1.5, color);
            },
        )
        .flex_1()
        .min_h_0()
        .w_full();

        let (track, plugin) = (input.track, effect.plugin.id);
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
            .flex_none()
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
        let controls = div()
            .flex()
            .flex_none()
            .gap(px(KNOB_GAP))
            .child(picker)
            .child(knobs);
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(graph)
            .child(controls)
            .into_any_element()
    }

    /// Its tail fading, and size, decay, damping, pre-delay and mix knobs.
    pub(super) fn reverb_body(
        &self,
        input: &EditorInput,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let effect = input.effect;
        let value = |name| effect.plugin.param(name).map_or(0., |param| param.get());
        let (decay, damping, predelay) =
            (value("decay"), value("damping"), value("predelay") / 1000.);
        let enabled = input.enabled();
        let paint_theme = theme.clone();
        let graph = canvas(
            |_, _, _| {},
            move |bounds, _, window, context| {
                let theme = &paint_theme;
                let rect = Rect::of(bounds);
                fill_rounded(window, rect, 2., theme.bg_deep);
                let inner = Rect::new(
                    rect.left + 4.,
                    rect.top + 4.,
                    rect.width - 8.,
                    rect.height - 8.,
                );
                let span = (predelay + decay) * 1.15;
                let x = |seconds: f32| inner.left + seconds / span * inner.width;
                let y = |db: f32| inner.top + (db / FLOOR_DB).clamp(0., 1.) * inner.height;
                for db in [20., 40.] {
                    hline(window, rect, y(db), theme.grid_beat);
                }
                let color = if enabled {
                    theme.accent
                } else {
                    theme.text_disabled
                };
                // Falls 60 dB over the decay, in a straight line in dB.
                let tail = |decay: f32| {
                    [
                        (x(0.), inner.bottom()),
                        (x(predelay), inner.bottom()),
                        (x(predelay), y(0.)),
                        (x(predelay + decay), y(FLOOR_DB)),
                    ]
                };
                let highs = decay * (1. - damping.clamp(0., 1.) * DAMPED_HIGHS);
                polygon(
                    window,
                    &[
                        (x(predelay), inner.bottom()),
                        (x(predelay), y(0.)),
                        (x(predelay + decay), y(FLOOR_DB)),
                    ],
                    faded(color, 0.15),
                );
                polyline(window, &tail(highs), 1., theme.text_muted);
                polyline(window, &tail(decay), 1.5, color);
                text(
                    window,
                    context,
                    format!("{} to -60 dB", format_seconds(decay)),
                    (rect.right() - 4., rect.top + 3.),
                    Align::RIGHT_TOP,
                    size::SMALL,
                    theme.text_muted,
                    None,
                );
            },
        )
        .flex_1()
        .min_h_0()
        .w_full();
        let knobs = self.knob_row(
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
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(graph)
            .child(knobs)
            .into_any_element()
    }
}
