//! The filter's editor: its response over the live spectrum, with a handle
//! for the frequency and resonance (or gain, for the shelves and peak)
//! dragged anywhere on it, the mode, and frequency, Q and gain knobs.

use std::{cell::Cell, ops::RangeInclusive, rc::Rc};

use gpui::{
    AnyElement, Bounds, ContentMask, Context, CursorStyle, MouseButton, MouseDownEvent, Pixels,
    canvas, div, prelude::*, px,
};
use tonique_engine::{
    dsp::Biquad,
    edit::{Parameter, PluginId, PluginKind, TrackId},
    meter::PluginTap,
    nodes::FilterMode,
};

use crate::{
    app::{PopupKind, Tonique},
    bottom::{
        paint::{Align, circle, curve, fill_rounded, text},
        rack::{
            RackDrag,
            editors::{Analyser, Editor, EditorInput},
            format_db, format_hz,
            graph::{freq_x, hline, paint_freq_grid, spectrum_points, x_freq},
            knob::KNOB_HEIGHT,
            shapes::shape_menu,
        },
    },
    core::effect::Setting,
    geometry::Rect,
    theme::{Theme, size, with_alpha},
    widgets::{Flat, Menu},
};

/// Gain range of the graph, in dB.
const TOP_DB: f32 = 24.;
const BOTTOM_DB: f32 = -36.;
/// Levels of the spectrum shown, in dBFS.
const SPECTRUM_RANGE: RangeInclusive<f32> = -90.0..=0.;
const HANDLE_RADIUS: f32 = 5.;
const MODE_WIDTH: f32 = 96.;

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
    gain: f32,
    sample_rate: f32,
    width: f32,
}

/// Shown in the mode's picker.
fn mode_name(mode: FilterMode) -> &'static str {
    match mode {
        FilterMode::LowPass => "Low-pass",
        FilterMode::HighPass => "High-pass",
        FilterMode::BandPass => "Band-pass",
        FilterMode::Notch => "Notch",
        FilterMode::LowShelf => "Low shelf",
        FilterMode::HighShelf => "High shelf",
        FilterMode::Peak => "Peak",
    }
}

/// The value of the handle's parameter at `db` up the graph: the gain
/// itself, or Q.
pub fn to_height(mode: FilterMode, db: f32) -> f32 {
    if mode.has_gain() {
        db
    } else {
        10f32.powf(db / 20.)
    }
}

/// Where the handle's parameter puts it up the graph, in dB.
pub fn from_height(mode: FilterMode, value: f32) -> f32 {
    if mode.has_gain() {
        value
    } else {
        20. * value.max(1e-3).log10()
    }
}

/// A spectrum of the audio in or out of an effect, under its graph.
pub struct LiveSpectrum {
    analyser: Analyser,
    pub tap: Tap,
}

impl LiveSpectrum {
    pub fn new() -> Self {
        Self {
            analyser: Analyser::new(),
            tap: Tap::Input,
        }
    }

    /// Takes in what the side of `tap` shown heard, keeping it recording
    /// for half a second past the last frame shown.
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

    /// Muted, behind the graph's curve.
    pub fn paint(&self, window: &mut gpui::Window, rect: Rect, theme: &Theme) {
        let points = spectrum_points(&self.analyser.spectrum, rect, &SPECTRUM_RANGE);
        curve(
            window,
            &points,
            rect.bottom(),
            (theme.text_muted, 1.),
            with_alpha(theme.text_disabled, 77),
        );
    }
}

pub struct FilterState {
    pub spectrum: LiveSpectrum,
    /// The response drawn last, a gain per pixel, and what it was for:
    /// worked out again only when that changes.
    response: (Option<ResponseKey>, Vec<f32>),
    /// The graph, as last laid out.
    pub graph: Rc<Cell<Bounds<Pixels>>>,
}

impl FilterState {
    pub fn new() -> Self {
        Self {
            spectrum: LiveSpectrum::new(),
            response: (None, Vec::new()),
            graph: Rc::default(),
        }
    }

    /// The filter's gain at each pixel across `scale`, in dB.
    fn response(&mut self, key: ResponseKey, scale: &Scale) -> &[f32] {
        if self.response.0 != Some(key) {
            let rect = scale.rect;
            let filter = Biquad::new(key.mode, key.cutoff, key.q, key.gain, key.sample_rate);
            self.response = (
                Some(key),
                (0..=key.width.ceil() as usize)
                    .map(|column| {
                        let hz = scale.hz(rect.left + column as f32);
                        filter.response_db(hz, key.sample_rate)
                    })
                    .collect(),
            );
        }
        &self.response.1
    }
}

/// Maps frequencies and gains to the graph and back.
pub struct Scale {
    pub rect: Rect,
    pub top_db: f32,
    pub bottom_db: f32,
}

impl Scale {
    /// The filter's graph in `rect`.
    pub fn filter(rect: Rect) -> Self {
        Self {
            rect,
            top_db: TOP_DB,
            bottom_db: BOTTOM_DB,
        }
    }

    pub fn x(&self, hz: f32) -> f32 {
        freq_x(self.rect, hz)
    }
    pub fn hz(&self, x: f32) -> f32 {
        x_freq(self.rect, x)
    }
    pub fn y(&self, db: f32) -> f32 {
        self.rect.top + (self.top_db - db) / (self.top_db - self.bottom_db) * self.rect.height
    }
    pub fn db(&self, y: f32) -> f32 {
        self.top_db - (y - self.rect.top) / self.rect.height * (self.top_db - self.bottom_db)
    }
}

/// A filter's handle being dragged: the pointer's x sets its frequency,
/// its y its height (the resonance, the gain at the cutoff of a low- or
/// high-pass, which is Q; or the gain of a shelf or peak).
pub struct GraphDrag {
    pub cutoff: Parameter,
    pub height: Parameter,
    pub mode: FilterMode,
    pub before: (f32, f32),
    pub graph: Rc<Cell<Bounds<Pixels>>>,
    /// Gains the graph spans, top and bottom.
    pub range: (f32, f32),
    /// A parameter set as the drag started (an EQ band switched on), and
    /// its value before: part of the drag's undo step.
    pub switched: Option<(Parameter, f32)>,
}

impl Tonique {
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
        let gain = effect.plugin.param("gain").cloned();
        // The handle's height: the gain for the shelves and peak, else the
        // resonance.
        let height = match &gain {
            Some(gain) if mode.has_gain() => gain.clone(),
            _ => q.clone(),
        };
        let plugin = effect.plugin.id;
        let enabled = input.enabled();
        let (graph_bounds, tap) = match &*input.editor.borrow() {
            Editor::Filter(filter) => (filter.graph.clone(), filter.spectrum.tap),
            _ => return div().into_any_element(),
        };
        let dragging = matches!(
            &self.bottom.rack.drag,
            Some(RackDrag::Graph(drag)) if drag.cutoff.id == cutoff.id
        );
        let initial = (
            effect.kind.initial_value(&effect.plugin, cutoff.name),
            effect.kind.initial_value(&effect.plugin, height.name),
        );

        let editor = input.editor.clone();
        let paint_theme = theme.clone();
        let (cutoff_value, q_value, height_value) = (cutoff.get(), q.get(), height.get());
        let gain_value = gain.as_ref().map_or(0., |gain| gain.get());
        let sample_rate = input.sample_rate;
        let slot = graph_bounds.clone();
        let painting = canvas(
            move |bounds, _, _| slot.set(bounds),
            move |bounds, _, window, context| {
                let theme = &paint_theme;
                let rect = Rect::of(bounds);
                let scale = Scale::filter(rect);
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
                    filter.spectrum.paint(window, rect, theme);
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
                    gain: gain_value,
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
                let handle = (
                    scale.x(cutoff_value),
                    scale.y(from_height(mode, height_value)),
                );
                let handle_fill = if active { color } else { theme.bg_deep };
                circle(
                    window,
                    handle,
                    HANDLE_RADIUS,
                    handle_fill,
                    Some((1.5, color)),
                );
                if active {
                    let detail = if mode.has_gain() {
                        format_db(gain_value)
                    } else {
                        format!("Q {q_value:.2}")
                    };
                    text(
                        window,
                        context,
                        format!("{} · {detail}", format_hz(cutoff_value)),
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

        let (press_cutoff, press_height) = (cutoff.clone(), height.clone());
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
                    let (cutoff, height) = (press_cutoff.clone(), press_height.clone());
                    let before = (cutoff.get(), height.get());
                    if event.click_count >= 2 {
                        this.bottom.rack.drag = None;
                        if let (Some(first), Some(second)) = initial {
                            cutoff.set(first);
                            height.set(second);
                            this.state.commit_params(&[
                                (cutoff.id, before.0, cutoff.get()),
                                (height.id, before.1, height.get()),
                            ]);
                        }
                    } else {
                        this.bottom.rack.drag = Some(RackDrag::Graph(GraphDrag {
                            cutoff,
                            height,
                            mode,
                            before,
                            graph: graph_bounds.clone(),
                            range: (TOP_DB, BOTTOM_DB),
                            switched: None,
                        }));
                    }
                    context.notify();
                }),
            );

        let tint = if enabled {
            theme.accent
        } else {
            theme.text_disabled
        };
        let picker = self.shape_picker(
            "filter-mode",
            mode,
            mode_name,
            MODE_WIDTH,
            tint,
            PopupKind::FilterShape(input.track, plugin),
            theme,
            context,
        );
        let knobs = div()
            .flex()
            .flex_none()
            .gap(px(6.))
            .h(px(KNOB_HEIGHT))
            .children(self.param_knob(effect, "cutoff", "Freq", format_hz, true, theme, context))
            .children(self.param_knob(
                effect,
                "q",
                "Q",
                |q| format!("{q:.2}"),
                true,
                theme,
                context,
            ))
            .when(mode.has_gain(), |knobs| {
                knobs.children(
                    self.param_knob(effect, "gain", "Gain", format_db, false, theme, context),
                )
            });
        let controls = div()
            .flex()
            .flex_none()
            .gap(px(6.))
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

    /// Over the graph: which side of the filter the spectrum shows.
    pub(super) fn tap_buttons(
        &self,
        plugin: PluginId,
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
                    if let Some(slot) = this.bottom.rack.slots.get(&plugin)
                        && let Some(spectrum) = slot.editor.borrow_mut().spectrum_mut()
                    {
                        spectrum.tap = tap;
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

    /// The filter's modes, for its picker, while the filter exists.
    pub fn filter_shape_menu(
        &self,
        track: TrackId,
        plugin: PluginId,
        context: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let effect = self
            .state
            .effects(&track)
            .into_iter()
            .find(|effect| effect.plugin.id == plugin)?;
        let PluginKind::Filter(mode) = effect.plugin.kind else {
            return None;
        };
        let menu = shape_menu(
            Menu::new(&self.theme),
            &FilterMode::ALL,
            mode,
            mode_name,
            self.theme.accent,
            context,
            move |this, picked| {
                this.state
                    .set_effect_setting(&track, plugin, Setting::Mode(picked));
            },
        );
        Some(menu.into_element())
    }

    /// Follows a filter's graph being dragged to `(x, y)`.
    pub(super) fn follow_graph(drag: &GraphDrag, x: f32, y: f32) {
        let scale = Scale {
            rect: Rect::of(drag.graph.get()),
            top_db: drag.range.0,
            bottom_db: drag.range.1,
        };
        if scale.rect.is_empty() {
            return;
        }
        if let Some((switch, _)) = &drag.switched {
            switch.set(1.);
        }
        drag.cutoff.set(scale.hz(x));
        drag.height.set(to_height(drag.mode, scale.db(y)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_map_back() {
        for mode in FilterMode::ALL {
            let db = from_height(mode, to_height(mode, 6.));
            assert!((db - 6.).abs() < 1e-3, "{mode:?}: {db}");
        }
        assert!(
            (to_height(FilterMode::LowPass, 0.) - 1.).abs() < 1e-6,
            "Q 1 at 0 dB"
        );
        assert_eq!(to_height(FilterMode::Peak, -3.), -3., "the gain itself");
    }

    #[test]
    fn scale_maps_back() {
        let scale = Scale::filter(Rect::new(10., 20., 300., 100.));
        assert!((scale.hz(scale.x(1234.)) - 1234.).abs() < 0.1);
        assert!((scale.db(scale.y(-7.)) + 7.).abs() < 1e-3);
        assert_eq!(scale.y(TOP_DB), 20.);
    }
}
