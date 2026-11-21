//! The controls of each kind of effect, as the egui app's editors. The
//! plugin's parameters are the source of truth: read each frame, set when
//! edited. What an editor keeps (an analyser's spectrum) is in its
//! [`Editor`], shared with the canvases drawing it.

use std::{cell::RefCell, rc::Rc, time::Instant};

use gpui::{AnyElement, Context, Stateful, canvas, div, prelude::*, px};
use tonique_engine::{
    edit::{PluginKind, TrackId},
    nodes::UTILITY_SILENT_DB,
    spectrum::Spectrum,
};

use crate::{
    app::Tonique,
    bottom::{
        paint::{Align, curve, fill, fill_rounded, text},
        rack::{
            filter::FilterState,
            format_hz, format_ms, format_percent,
            graph::{hline, level_y, paint_freq_grid, spectrum_points, x_freq},
            knob::{KNOB_HEIGHT, Knob, KnobScale, KnobTarget},
        },
    },
    core::effect::{ECHO_TIME, ECHO_TIMES, Effect, EffectKind},
    geometry::Rect,
    theme::{Theme, size},
    widgets::Flat,
};

/// Size of a parameter's switch.
const TOGGLE_SIZE: (f32, f32) = (30., 20.);
/// Repeats an echo draws, at most.
const REPEATS: usize = 12;
/// Levels a spectrum shows, in dBFS, and between its level lines.
const SPECTRUM_RANGE: std::ops::RangeInclusive<f32> = -96.0..=0.;
const LEVEL_STEP: f32 = 12.;

/// What an editor keeps between frames.
pub enum Editor {
    Filter(FilterState),
    Echo,
    Spectrum(Analyser),
    Utility,
}

/// A spectrum, smoothed frame after frame.
pub struct Analyser {
    pub spectrum: Spectrum,
    last: Option<Instant>,
}

impl Analyser {
    pub fn new() -> Self {
        Self {
            spectrum: Spectrum::new(),
            last: None,
        }
    }

    /// Takes in what `meter` heard since the last frame.
    pub fn update(&mut self, meter: &tonique_engine::meter::ChannelMeter, sample_rate: f32) {
        let now = Instant::now();
        let seconds = self
            .last
            .map_or(1. / 60., |last| (now - last).as_secs_f32().min(0.1));
        self.last = Some(now);
        self.spectrum.update(meter, sample_rate, seconds);
    }
}

impl Editor {
    pub fn new(kind: EffectKind) -> Self {
        match kind {
            EffectKind::Filter => Self::Filter(FilterState::new()),
            EffectKind::Echo => Self::Echo,
            EffectKind::Spectrum => Self::Spectrum(Analyser::new()),
            EffectKind::Utility => Self::Utility,
        }
    }

    /// Width of the editor's frame.
    pub fn width(kind: EffectKind) -> f32 {
        match kind {
            EffectKind::Filter => 280.,
            EffectKind::Echo => 200.,
            EffectKind::Spectrum => 320.,
            EffectKind::Utility => 172.,
        }
    }

    /// Whether it shows something moving on its own.
    pub fn is_animated(&self) -> bool {
        matches!(self, Self::Filter(_) | Self::Spectrum(_))
    }

    /// Takes in the audio of `effect` its graph shows, keeping its tap
    /// recording for half a second past the last frame shown.
    pub fn update(&mut self, effect: &Effect, sample_rate: f32) {
        if !effect.enabled() {
            return;
        }
        let tap = &effect.plugin.tap;
        let frames = (sample_rate / 2.) as u32;
        match self {
            Self::Filter(filter) => filter.update(tap, sample_rate),
            Self::Spectrum(analyser) => {
                tap.watch_output_for(frames);
                analyser.update(&tap.output, sample_rate);
            }
            Self::Echo | Self::Utility => {}
        }
    }
}

/// What building an editor needs.
pub struct EditorInput<'a> {
    pub effect: &'a Effect,
    pub track: TrackId,
    pub sample_rate: f32,
    pub editor: Rc<RefCell<Editor>>,
    /// A time being dragged, not applied yet.
    pub pending: Option<f32>,
}

impl EditorInput<'_> {
    pub fn enabled(&self) -> bool {
        self.effect.enabled()
    }
}

impl Tonique {
    /// The controls of `input`'s effect, filling its frame's body.
    pub(super) fn editor_body(
        &self,
        input: &EditorInput,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        match input.effect.kind {
            EffectKind::Filter => self.filter_body(input, theme, context),
            EffectKind::Echo => self.echo_body(input, theme, context),
            EffectKind::Spectrum => spectrum_body(input, theme),
            EffectKind::Utility => self.utility_body(input, theme, context),
        }
    }

    /// A knob for the parameter `param` of `effect`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn param_knob(
        &self,
        effect: &Effect,
        param: &'static str,
        name: &'static str,
        format: fn(f32) -> String,
        log: bool,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> Option<Stateful<gpui::Div>> {
        let parameter = effect.plugin.param(param)?.clone();
        let value = parameter.get();
        let knob = Knob {
            name,
            value,
            scale: KnobScale {
                min: parameter.min,
                max: parameter.max,
                log,
            },
            text: format(value),
            default: effect.kind.initial_value(&effect.plugin, param),
            active: effect.enabled(),
        };
        Some(self.knob_view(param, knob, KnobTarget::Param(parameter), theme, context))
    }

    /// A button switching a 0/1 parameter, as an undo step.
    fn param_toggle(
        &self,
        effect: &Effect,
        param: &'static str,
        name: &'static str,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> Option<Stateful<gpui::Div>> {
        let parameter = effect.plugin.param(param)?.clone();
        let on = parameter.get() >= 0.5;
        let label = div().text_size(px(size::SMALL)).child(name);
        Some(
            Flat::new(param, TOGGLE_SIZE.1)
                .width(TOGGLE_SIZE.0)
                .selected(on)
                .show(label, theme, context, move |this, _| {
                    let (old, new) = (parameter.get(), if on { 0. } else { 1. });
                    parameter.set(new);
                    this.state.commit_params(&[(parameter.id, old, new)]);
                }),
        )
    }

    /// The repeats fading out, and time, feedback and mix knobs.
    fn echo_body(
        &self,
        input: &EditorInput,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let effect = input.effect;
        let PluginKind::Echo { time_s } = effect.plugin.kind else {
            return div().into_any_element();
        };
        let feedback = effect.plugin.param("feedback").map_or(0., |p| p.get());
        let mix = effect.plugin.param("mix").map_or(0., |p| p.get());
        let enabled = input.enabled();
        let graph_theme = theme.clone();
        let graph = canvas(
            |_, _, _| {},
            move |bounds, _, window, context| {
                let rect = Rect::of(bounds);
                let theme = &graph_theme;
                fill_rounded(window, rect, 2., theme.bg_deep);
                let color = if enabled {
                    theme.accent
                } else {
                    theme.text_disabled
                };
                // The dry signal, then each repeat a delay later, quieter by
                // the feedback.
                let inner = Rect::new(
                    rect.left + 8.,
                    rect.top + 6.,
                    rect.width - 16.,
                    rect.height - 12.,
                );
                let step = inner.width / REPEATS as f32;
                let mut level = mix;
                for repeat in 0..=REPEATS {
                    let (height, color) = if repeat == 0 {
                        (1., theme.text_muted)
                    } else {
                        let height = level;
                        level *= feedback;
                        (height, color)
                    };
                    if height < 0.01 {
                        break;
                    }
                    let x = inner.left + repeat as f32 * step;
                    let tall = height * inner.height;
                    fill(
                        window,
                        Rect::new(x - 1.5, inner.bottom() - tall, 3., tall),
                        color,
                    );
                }
                text(
                    window,
                    context,
                    format!("{} per repeat", format_ms(time_s)),
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

        let shown_time = input.pending.unwrap_or(time_s);
        let time = self.knob_view(
            "time",
            Knob {
                name: "Time",
                value: shown_time,
                scale: KnobScale {
                    min: *ECHO_TIMES.start(),
                    max: *ECHO_TIMES.end(),
                    log: true,
                },
                text: format_ms(shown_time),
                default: Some(ECHO_TIME),
                active: enabled,
            },
            KnobTarget::Time {
                track: input.track,
                plugin: effect.plugin.id,
            },
            theme,
            context,
        );
        let knobs = div()
            .flex()
            .flex_none()
            .gap(px(6.))
            .h(px(KNOB_HEIGHT))
            .child(time)
            .children(self.param_knob(
                effect,
                "feedback",
                "Feedback",
                format_percent,
                false,
                theme,
                context,
            ))
            .children(self.param_knob(effect, "mix", "Mix", format_percent, false, theme, context));
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(graph)
            .child(knobs)
            .into_any_element()
    }

    /// Gain, balance and width knobs, and mono and phase switches, centred.
    /// Its level shows on the meter every effect has.
    fn utility_body(
        &self,
        input: &EditorInput,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let effect = input.effect;
        let knobs = div()
            .flex()
            .gap(px(6.))
            .children(self.param_knob(effect, "gain", "Gain", format_gain, false, theme, context))
            .children(self.param_knob(
                effect,
                "balance",
                "Balance",
                format_balance,
                false,
                theme,
                context,
            ))
            .children(self.param_knob(
                effect,
                "width",
                "Width",
                |width| format!("{:.0}%", width * 100.),
                false,
                theme,
                context,
            ));
        let toggles = div()
            .flex()
            .gap(px(2.))
            .children(self.param_toggle(effect, "mono", "Mono", theme, context))
            .children(self.param_toggle(effect, "invert_left", "Ø L", theme, context))
            .children(self.param_toggle(effect, "invert_right", "Ø R", theme, context));
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .child(knobs)
            .child(toggles)
            .into_any_element()
    }
}

/// The spectrum of the audio where it sits in the chain, with a readout of
/// the frequency and level under the pointer.
fn spectrum_body(input: &EditorInput, theme: &Theme) -> AnyElement {
    let enabled = input.enabled();
    let editor = input.editor.clone();
    let theme = theme.clone();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, context| {
            let rect = Rect::of(bounds);
            fill_rounded(window, rect, 2., theme.bg_deep);
            paint_freq_grid(window, context, rect, &theme);
            let mut db = *SPECTRUM_RANGE.end() - LEVEL_STEP;
            while db > *SPECTRUM_RANGE.start() {
                let y = level_y(rect, &SPECTRUM_RANGE, db);
                hline(window, rect, y, theme.grid_beat);
                text(
                    window,
                    context,
                    format!("{db:.0}"),
                    (rect.right() - 2., y - 1.),
                    Align::RIGHT_BOTTOM,
                    size::SMALL,
                    theme.text_muted,
                    None,
                );
                db -= LEVEL_STEP;
            }
            if !enabled {
                text(
                    window,
                    context,
                    "Bypassed",
                    (rect.left + rect.width / 2., rect.top + rect.height / 2.),
                    Align::CENTER,
                    size::SMALL,
                    theme.text_muted,
                    None,
                );
                return;
            }
            let editor = editor.borrow();
            let Editor::Spectrum(analyser) = &*editor else {
                return;
            };
            let points = spectrum_points(&analyser.spectrum, rect, &SPECTRUM_RANGE);
            curve(
                window,
                &points,
                rect.bottom(),
                (theme.accent, 1.5),
                crate::theme::with_alpha(theme.accent, 38),
            );
            let pointer = window.mouse_position();
            if bounds.contains(&pointer) {
                let x = f32::from(pointer.x);
                let hz = x_freq(rect, x);
                let level = analyser.spectrum.level(hz);
                fill(
                    window,
                    Rect::new(x.round(), rect.top, 1., rect.height),
                    theme.text_muted,
                );
                crate::bottom::paint::circle(
                    window,
                    (x, level_y(rect, &SPECTRUM_RANGE, level)),
                    3.,
                    theme.text,
                    None,
                );
                text(
                    window,
                    context,
                    format!("{}  {level:.1} dB", format_hz(hz)),
                    (rect.left + 4., rect.top + 3.),
                    Align::LEFT_TOP,
                    size::SMALL,
                    theme.text,
                    None,
                );
            }
        },
    )
    .size_full()
    .into_any_element()
}

pub fn format_gain(db: f32) -> String {
    if db <= UTILITY_SILENT_DB {
        "-inf dB".into()
    } else {
        format!("{db:+.1} dB")
    }
}

/// `C` in the middle, else how far to a side, from 1 to 50.
pub fn format_balance(balance: f32) -> String {
    let amount = (balance.abs() * 50.).round();
    if amount == 0. {
        "C".into()
    } else if balance < 0. {
        format!("{amount:.0}L")
    } else {
        format!("{amount:.0}R")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_as_usual() {
        assert_eq!(format_gain(UTILITY_SILENT_DB), "-inf dB");
        assert_eq!(format_gain(-6.), "-6.0 dB");
        assert_eq!(format_gain(3.), "+3.0 dB");
        assert_eq!(format_balance(0.001), "C");
        assert_eq!(format_balance(-1.), "50L");
        assert_eq!(format_balance(0.3), "15R");
    }
}
