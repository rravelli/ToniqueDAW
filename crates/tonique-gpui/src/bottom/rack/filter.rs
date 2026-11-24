//! The filter's editor: its response over the live spectrum, with a handle
//! for cutoff and resonance dragged anywhere on it, the mode, and cutoff and
//! Q knobs.

use crate::bottom::BottomPanel;
use crate::model::HoldsProject;
use std::{cell::Cell, f32::consts::PI, ops::RangeInclusive, rc::Rc};

use gpui::{
    AnyElement, Bounds, ContentMask, Context, CursorStyle, MouseButton, MouseDownEvent, Pixels,
    canvas, div, prelude::*, px,
};
use tonique_engine::{
    edit::{Parameter, PluginKind},
    meter::PluginTap,
    nodes::FilterMode,
};

use crate::{
    bottom::{
        paint::{Align, circle, curve, fill_rounded, text},
        rack::{
            RackDrag,
            editors::{Analyser, Editor, EditorInput},
            format_hz,
            graph::{freq_x, hline, paint_freq_grid, spectrum_points, x_freq},
            knob::KNOB_HEIGHT,
        },
    },
    geometry::Rect,
    theme::{Theme, size, with_alpha},
    widgets::Flat,
};
use tonique_core::document::effect::Setting;

/// Gain range of the graph, in dB.
const TOP_DB: f32 = 24.;
const BOTTOM_DB: f32 = -36.;
/// Levels of the spectrum shown, in dBFS.
const SPECTRUM_RANGE: RangeInclusive<f32> = -90.0..=0.;
const HANDLE_RADIUS: f32 = 5.;

/// Which side of the filter the spectrum shows.
#[derive(Clone, Copy, PartialEq)]
pub enum Tap {
    Input,
    Output,
}

/// What a filter's drawn response depends on.
#[derive(Clone, Copy, PartialEq)]
struct ResponseKey {
    mode: FilterMode,
    cutoff: f32,
    q: f32,
    sample_rate: f32,
    width: f32,
}

pub struct FilterState {
    analyser: Analyser,
    tap: Tap,
    /// The response drawn last, a gain per pixel, and what it was for:
    /// worked out again only when that changes.
    response: (Option<ResponseKey>, Vec<f32>),
    /// The graph, as last laid out.
    pub graph: Rc<Cell<Bounds<Pixels>>>,
}

impl FilterState {
    pub fn new() -> Self {
        Self {
            analyser: Analyser::new(),
            tap: Tap::Input,
            response: (None, Vec::new()),
            graph: Rc::default(),
        }
    }

    pub fn update(&mut self, tap: &PluginTap, sample_rate: f32) {
        let frames = (sample_rate / 2.) as u32;
        let meter = match self.tap {
            Tap::Input => {
                tap.watch_input_for(frames);
                &tap.input
            }
            Tap::Output => {
                tap.watch_output_for(frames);
                &tap.output
            }
        };
        self.analyser.update(meter, sample_rate);
    }

    /// The filter's gain at each pixel across `scale`, in dB.
    fn response(&mut self, key: ResponseKey, scale: &Scale) -> &[f32] {
        if self.response.0 != Some(key) {
            let rect = scale.rect;
            self.response = (
                Some(key),
                (0..=key.width.ceil() as usize)
                    .map(|column| {
                        let hz = scale.hz(rect.left + column as f32);
                        response_db(key.mode, key.cutoff, key.q, key.sample_rate, hz)
                    })
                    .collect(),
            );
        }
        &self.response.1
    }
}

/// Gain of the engine's filter at `hz`, in dB. Mirrors `FilterNode`'s RBJ
/// biquad, so the curve shows what's heard.
pub fn response_db(mode: FilterMode, cutoff: f32, q: f32, sample_rate: f32, hz: f32) -> f32 {
    let frequency = cutoff.clamp(10., sample_rate * 0.49);
    let w0 = 2. * PI * frequency / sample_rate;
    let (sin0, cos0) = w0.sin_cos();
    let alpha = sin0 / (2. * q.max(0.05));
    let (b0, b1, b2) = match mode {
        FilterMode::LowPass => ((1. - cos0) / 2., 1. - cos0, (1. - cos0) / 2.),
        FilterMode::HighPass => ((1. + cos0) / 2., -(1. + cos0), (1. + cos0) / 2.),
    };
    let (a0, a1, a2) = (1. + alpha, -2. * cos0, 1. - alpha);
    let w = 2. * PI * hz / sample_rate;
    let magnitude = |c0: f32, c1: f32, c2: f32| {
        let real = c0 + c1 * w.cos() + c2 * (2. * w).cos();
        let imaginary = -(c1 * w.sin() + c2 * (2. * w).sin());
        (real * real + imaginary * imaginary).sqrt()
    };
    20. * (magnitude(b0, b1, b2) / magnitude(a0, a1, a2))
        .max(1e-9)
        .log10()
}

/// Maps frequencies and gains to the graph and back.
pub struct Scale {
    pub rect: Rect,
}

impl Scale {
    fn x(&self, hz: f32) -> f32 {
        freq_x(self.rect, hz)
    }
    pub fn hz(&self, x: f32) -> f32 {
        x_freq(self.rect, x)
    }
    fn y(&self, db: f32) -> f32 {
        self.rect.top + (TOP_DB - db) / (TOP_DB - BOTTOM_DB) * self.rect.height
    }
    pub fn db(&self, y: f32) -> f32 {
        TOP_DB - (y - self.rect.top) / self.rect.height * (TOP_DB - BOTTOM_DB)
    }

    /// The cutoff and Q the handle at `(x, y)` sets: the resonance is the
    /// gain at the cutoff.
    pub fn handle_values(&self, x: f32, y: f32) -> (f32, f32) {
        (self.hz(x), 10f32.powf(self.db(y) / 20.))
    }
}

/// The filter's graph being dragged.
pub struct GraphDrag {
    pub cutoff: Parameter,
    pub q: Parameter,
    pub before: (f32, f32),
    pub graph: Rc<Cell<Bounds<Pixels>>>,
}

impl BottomPanel {
    pub(super) fn filter_body(
        &self,
        input: &EditorInput,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let effect = input.effect;
        let PluginKind::Filter(mode) = effect.plugin.kind else {
            return div().into_any_element();
        };
        let (Some(cutoff), Some(q)) = (
            effect.plugin.param("cutoff").cloned(),
            effect.plugin.param("q").cloned(),
        ) else {
            return div().into_any_element();
        };
        let plugin = effect.plugin.id;
        let enabled = input.enabled();
        let (graph_bounds, tap) = match &*input.editor.borrow() {
            Editor::Filter(filter) => (filter.graph.clone(), filter.tap),
            _ => return div().into_any_element(),
        };
        let dragging = matches!(
            &self.rack.drag,
            Some(RackDrag::Graph(drag)) if drag.cutoff.id == cutoff.id
        );
        let initial = (
            effect.kind.initial_value(&effect.plugin, "cutoff"),
            effect.kind.initial_value(&effect.plugin, "q"),
        );

        let editor = input.editor.clone();
        let paint_theme = theme.clone();
        let (cutoff_value, q_value) = (cutoff.get(), q.get());
        let sample_rate = input.sample_rate;
        let slot = graph_bounds.clone();
        let painting = canvas(
            move |bounds, _, _| slot.set(bounds),
            move |bounds, _, window, context| {
                let theme = &paint_theme;
                let rect = Rect::of(bounds);
                let scale = Scale { rect };
                let active = dragging || bounds.contains(&window.mouse_position());
                fill_rounded(window, rect, 2., theme.bg_deep);
                paint_freq_grid(window, context, rect, theme);
                let mut db = (BOTTOM_DB / 12.).ceil() * 12.;
                while db < TOP_DB {
                    let color = if db == 0. {
                        theme.grid_bar
                    } else {
                        theme.grid_beat
                    };
                    hline(window, rect, scale.y(db), color);
                    db += 12.;
                }
                let mut editor = editor.borrow_mut();
                let Editor::Filter(filter) = &mut *editor else {
                    return;
                };
                if enabled {
                    let points = spectrum_points(&filter.analyser.spectrum, rect, &SPECTRUM_RANGE);
                    curve(
                        window,
                        &points,
                        rect.bottom(),
                        (theme.text_muted, 1.),
                        with_alpha(theme.text_disabled, 77),
                    );
                }
                let color = if enabled {
                    theme.accent
                } else {
                    theme.text_disabled
                };
                let key = ResponseKey {
                    mode,
                    cutoff: cutoff_value,
                    q: q_value,
                    sample_rate,
                    width: rect.width,
                };
                let points: Vec<(f32, f32)> = filter
                    .response(key, &scale)
                    .iter()
                    .enumerate()
                    .map(|(column, db)| (rect.left + column as f32, scale.y(*db)))
                    .collect();
                // The gain goes far past the graph's range: clip its line
                // to the graph.
                let mask = ContentMask { bounds };
                window.with_content_mask(Some(mask), |window| {
                    curve(
                        window,
                        &points,
                        rect.bottom(),
                        (color, 1.5),
                        with_alpha(color, 38),
                    );
                });
                let handle = (scale.x(cutoff_value), scale.y(20. * q_value.log10()));
                let handle_fill = if active { color } else { theme.bg_deep };
                circle(
                    window,
                    handle,
                    HANDLE_RADIUS,
                    handle_fill,
                    Some((1.5, color)),
                );
                if active {
                    text(
                        window,
                        context,
                        format!("{} · Q {q_value:.2}", format_hz(cutoff_value)),
                        (rect.right() - 4., rect.top + 3.),
                        Align::RIGHT_TOP,
                        size::SMALL,
                        theme.text,
                        None,
                    );
                }
            },
        )
        .absolute()
        .size_full();

        let (press_cutoff, press_q) = (cutoff.clone(), q.clone());
        let graph = div()
            .id("filter-graph")
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .cursor(CursorStyle::Crosshair)
            .child(painting)
            .when(enabled, |element| {
                element.child(self.tap_buttons(plugin, tap, theme, context))
            })
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, event: &MouseDownEvent, _, context| {
                    context.stop_propagation();
                    let (cutoff, q) = (press_cutoff.clone(), press_q.clone());
                    let before = (cutoff.get(), q.get());
                    if event.click_count >= 2 {
                        this.rack.drag = None;
                        if let (Some(first), Some(second)) = initial {
                            cutoff.set(first);
                            q.set(second);
                            this.state_mut(context).commit_params(&[
                                (cutoff.id, before.0, cutoff.get()),
                                (q.id, before.1, q.get()),
                            ]);
                        }
                    } else {
                        this.rack.drag = Some(RackDrag::Graph(GraphDrag {
                            cutoff,
                            q,
                            before,
                            graph: graph_bounds.clone(),
                        }));
                    }
                    context.notify();
                }),
            );

        let modes =
            [(FilterMode::LowPass, "LP"), (FilterMode::HighPass, "HP")].map(|(option, name)| {
                let track = input.track;
                Flat::new(name, 18.)
                    .width(28.)
                    .selected(mode == option)
                    .show(
                        div().text_size(px(size::SMALL)).child(name),
                        theme,
                        context,
                        move |this, context| {
                            if mode != option {
                                this.state_mut(context).set_effect_setting(
                                    &track,
                                    plugin,
                                    Setting::Mode(option),
                                );
                            }
                        },
                    )
            });
        let controls = div()
            .flex()
            .flex_none()
            .gap(px(6.))
            .h(px(KNOB_HEIGHT))
            .child(div().flex().flex_col().gap(px(2.)).children(modes))
            .children(self.param_knob(effect, "cutoff", "Freq", format_hz, true, theme, context))
            .children(self.param_knob(
                effect,
                "q",
                "Q",
                |q| format!("{q:.2}"),
                true,
                theme,
                context,
            ));
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(graph)
            .child(controls)
            .into_any_element()
    }

    /// Over the graph: which side of the filter the spectrum shows.
    fn tap_buttons(
        &self,
        plugin: tonique_engine::edit::PluginId,
        shown: Tap,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let buttons = [(Tap::Input, "Pre"), (Tap::Output, "Post")].map(|(tap, name)| {
            Flat::new(name, 14.).width(26.).selected(shown == tap).show(
                div().text_size(px(size::SMALL)).child(name),
                theme,
                context,
                move |this, _| {
                    if let Some(slot) = this.rack.slots.get(&plugin)
                        && let Editor::Filter(filter) = &mut *slot.editor.borrow_mut()
                    {
                        filter.tap = tap;
                    }
                },
            )
        });
        div()
            .absolute()
            .top(px(3.))
            .left(px(3.))
            .flex()
            .gap(px(2.))
            .children(buttons)
    }

    /// Follows the filter's graph being dragged to `(x, y)`.
    pub(super) fn follow_graph(drag: &GraphDrag, x: f32, y: f32) {
        let scale = Scale {
            rect: Rect::of(drag.graph.get()),
        };
        if scale.rect.is_empty() {
            return;
        }
        let (cutoff, q) = scale.handle_values(x, y);
        drag.cutoff.set(cutoff);
        drag.q.set(q);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 48_000.;

    #[test]
    fn response_matches_the_filter() {
        for mode in [FilterMode::LowPass, FilterMode::HighPass] {
            // The gain at the cutoff is Q.
            let db = response_db(mode, 1000., 4., SAMPLE_RATE, 1000.);
            assert!((db - 20. * 4f32.log10()).abs() < 0.01, "{db}");
        }
        let low = |hz| response_db(FilterMode::LowPass, 1000., 0.707, SAMPLE_RATE, hz);
        assert!(low(20.).abs() < 0.01, "passes the lows");
        assert!(low(10_000.) < -35., "cuts the highs: {}", low(10_000.));
    }

    #[test]
    fn scale_maps_back() {
        let scale = Scale {
            rect: Rect::new(10., 20., 300., 100.),
        };
        assert!((scale.hz(scale.x(1234.)) - 1234.).abs() < 0.1);
        assert!((scale.db(scale.y(-7.)) + 7.).abs() < 1e-3);
        assert_eq!(scale.y(TOP_DB), 20.);
    }
}
