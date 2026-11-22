//! The EQ's editor, as the egui app's: the bands' curve, a point per band
//! switched on, dragged for its frequency and gain (or resonance), added
//! and removed by double-clicks; the bands' numbers, and the picked band's
//! switch, shape and knobs.

use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, Bounds, ContentMask, Context, CursorStyle, MouseButton, MouseDownEvent, Pixels,
    canvas, div, prelude::*, px,
};
use tonique_engine::{
    dsp::Biquad,
    edit::Parameter,
    nodes::{EQ_BANDS, EQ_GAIN_DB, FilterMode, eq_mode, eq_mode_value},
};

use crate::{
    app::Tonique,
    bottom::{
        paint::{Align, circle, curve, fill_rounded, text},
        rack::{
            RackDrag,
            editors::{Editor, EditorInput, KNOB_GAP},
            filter::{GraphDrag, Scale, from_height},
            format_db, format_hz,
            graph::{hline, paint_freq_grid},
            knob::{KNOB_HEIGHT, KNOB_WIDTH},
        },
    },
    core::effect::Effect,
    geometry::Rect,
    theme::{Color, Theme, size, with_alpha},
    widgets::Flat,
};

const BANDS: usize = EQ_BANDS.len();
/// Gain range of the graph, in dB, either side of 0.
const RANGE_DB: f32 = EQ_GAIN_DB + 6.;
const POINT_RADIUS: f32 = 6.5;
/// How near a point a press must be to take it.
const GRAB_RADIUS: f32 = 10.;
/// A point added below this is a low cut, above the other a high cut.
const LOW_CUT_BELOW: f32 = 40.;
const HIGH_CUT_ABOVE: f32 = 15_000.;
/// A band's number button.
const NUMBER_SIZE: f32 = 20.;
const SHAPE_WIDTH: f32 = 70.;

/// A band's frequency, gain, Q, shape and switch.
type BandParams = [Parameter; 5];

/// A band's parameters, as they are now.
#[derive(Clone, Copy, PartialEq)]
struct BandValues {
    hz: f32,
    gain: f32,
    q: f32,
    mode: FilterMode,
    on: bool,
}

impl BandValues {
    fn of(params: &BandParams) -> Self {
        let [hz, gain, q, shape, on] = params;
        Self {
            hz: hz.get(),
            gain: gain.get(),
            q: q.get(),
            mode: eq_mode(shape.get()),
            on: on.get() >= 0.5,
        }
    }

    /// Where its point is up the graph, in dB.
    fn height(&self) -> f32 {
        from_height(
            self.mode,
            if self.mode.has_gain() {
                self.gain
            } else {
                self.q
            },
        )
    }
}

/// What the EQ's editor keeps.
pub struct EqState {
    /// The band whose knobs show.
    selected: usize,
    /// The graph, as last laid out.
    graph: Rc<Cell<Bounds<Pixels>>>,
}

impl EqState {
    pub fn new() -> Self {
        Self {
            selected: 0,
            graph: Rc::default(),
        }
    }
}

/// Shown on a band's shape button.
fn shape_name(mode: FilterMode) -> &'static str {
    match mode {
        FilterMode::Peak => "Bell",
        FilterMode::LowShelf => "Low shelf",
        FilterMode::HighShelf => "High shelf",
        FilterMode::HighPass => "Low cut",
        FilterMode::LowPass => "High cut",
        FilterMode::Notch => "Notch",
        FilterMode::BandPass => "Band-pass",
    }
}

/// Each band's own colour: spread over the theme's palette so no two
/// bands share one.
fn band_color(theme: &Theme, band: usize, enabled: bool) -> Color {
    if !enabled {
        return theme.text_disabled;
    }
    match theme.palette.len() {
        0 => theme.accent,
        count => theme.palette[band * count / BANDS % count],
    }
}

fn band_params(effect: &Effect) -> Option<[BandParams; BANDS]> {
    let mut bands = Vec::with_capacity(BANDS);
    for band in &EQ_BANDS {
        let mut params = Vec::with_capacity(5);
        for name in band.params {
            params.push(effect.plugin.param(name)?.clone());
        }
        bands.push(params.try_into().ok()?);
    }
    bands.try_into().ok()
}

/// The parameter a band's point sets up the graph: its gain, or Q.
fn height_param(params: &BandParams) -> &Parameter {
    let [_, gain, q, shape, _] = params;
    if eq_mode(shape.get()).has_gain() {
        gain
    } else {
        q
    }
}

fn graph_scale(rect: Rect) -> Scale {
    Scale {
        rect,
        top_db: RANGE_DB,
        bottom_db: -RANGE_DB,
    }
}

impl Tonique {
    pub(super) fn eq_body(
        &self,
        input: &EditorInput,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let effect = input.effect;
        let Some(params) = band_params(effect) else {
            return div().into_any_element();
        };
        let (selected, graph_bounds) = match &*input.editor.borrow() {
            Editor::Eq(eq) => (eq.selected, eq.graph.clone()),
            _ => return div().into_any_element(),
        };
        let values = params.each_ref().map(BandValues::of);
        let enabled = input.enabled();
        let sample_rate = input.sample_rate;
        let paint_theme = theme.clone();
        let slot = graph_bounds.clone();
        let painting = canvas(
            move |bounds, _, _| slot.set(bounds),
            move |bounds, _, window, context| {
                let theme = &paint_theme;
                let rect = Rect::of(bounds);
                let scale = graph_scale(rect);
                fill_rounded(window, rect, 2., theme.bg_deep);
                paint_freq_grid(window, context, rect, theme);
                for db in [-12., 0., 12.] {
                    let color = if db == 0. {
                        theme.grid_bar
                    } else {
                        theme.grid_beat
                    };
                    hline(window, rect, scale.y(db), color);
                }
                let filters: Vec<Biquad> = values
                    .iter()
                    .filter(|band| band.on)
                    .map(|band| Biquad::new(band.mode, band.hz, band.q, band.gain, sample_rate))
                    .collect();
                let points: Vec<(f32, f32)> = (0..=rect.width.ceil() as usize)
                    .map(|column| {
                        let x = rect.left + column as f32;
                        let hz = scale.hz(x);
                        let db: f32 = filters
                            .iter()
                            .map(|filter| filter.response_db(hz, sample_rate))
                            .sum();
                        (x, scale.y(db))
                    })
                    .collect();
                let color = if enabled {
                    theme.accent
                } else {
                    theme.text_disabled
                };
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    curve(
                        window,
                        &points,
                        scale.y(0.),
                        (color, 1.5),
                        with_alpha(color, 38),
                    );
                });
                for (band, value) in values.iter().enumerate() {
                    if !value.on && band != selected {
                        continue;
                    }
                    let at = (scale.x(value.hz), scale.y(value.height()));
                    let band_color = band_color(theme, band, enabled && value.on);
                    let (fill, number) = if value.on {
                        (band_color, theme.bg_deep)
                    } else {
                        (theme.bg_deep, band_color)
                    };
                    let outline = if band == selected {
                        theme.text
                    } else {
                        band_color
                    };
                    circle(window, at, POINT_RADIUS, fill, Some((1.5, outline)));
                    text(
                        window,
                        context,
                        (band + 1).to_string(),
                        at,
                        Align::CENTER,
                        size::SMALL,
                        number,
                        None,
                    );
                }
                let picked = values[selected];
                let detail = if picked.mode.has_gain() {
                    format_db(picked.gain)
                } else {
                    format!("Q {:.2}", picked.q)
                };
                text(
                    window,
                    context,
                    format!("{} · {} · {detail}", selected + 1, format_hz(picked.hz)),
                    (rect.right() - 4., rect.top + 3.),
                    Align::RIGHT_TOP,
                    size::SMALL,
                    theme.text_muted,
                    None,
                );
            },
        )
        .absolute()
        .size_full();

        let editor = input.editor.clone();
        let press_params = params.clone();
        let graph = div()
            .id("eq-graph")
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .cursor(CursorStyle::Crosshair)
            .child(painting)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, event: &MouseDownEvent, _, context| {
                    context.stop_propagation();
                    let graph = graph_bounds.get();
                    let scale = graph_scale(Rect::of(graph));
                    if scale.rect.is_empty() {
                        return;
                    }
                    let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
                    let now = press_params.each_ref().map(BandValues::of);
                    let Editor::Eq(eq) = &mut *editor.borrow_mut() else {
                        return;
                    };
                    let nearest = (0..BANDS)
                        .filter(|band| now[*band].on || *band == eq.selected)
                        .map(|band| {
                            let point = (scale.x(now[band].hz), scale.y(now[band].height()));
                            (band, (point.0 - x).hypot(point.1 - y))
                        })
                        .filter(|(_, distance)| *distance <= GRAB_RADIUS)
                        .min_by(|first, second| first.1.total_cmp(&second.1))
                        .map(|(band, _)| band);
                    this.bottom.rack.drag = None;
                    if event.click_count >= 2 {
                        match nearest {
                            // Removed: switched off.
                            Some(band) => {
                                let on = &press_params[band][4];
                                let before = on.get();
                                on.set(0.);
                                this.state.commit_params(&[(on.id, before, 0.)]);
                            }
                            None => {
                                if let Some(band) =
                                    add_point(this, &press_params, &now, scale.hz(x), scale.db(y))
                                {
                                    eq.selected = band;
                                }
                            }
                        }
                    } else if let Some(band) = nearest {
                        // Dragged across for its frequency, up for its
                        // height; a band that's off switches on.
                        eq.selected = band;
                        let band_params = &press_params[band];
                        let height = height_param(band_params).clone();
                        let on = band_params[4].clone();
                        let switched = Some((on.clone(), on.get()));
                        on.set(1.);
                        this.bottom.rack.drag = Some(RackDrag::Graph(GraphDrag {
                            cutoff: band_params[0].clone(),
                            before: (band_params[0].get(), height.get()),
                            height,
                            mode: now[band].mode,
                            graph: graph_bounds.clone(),
                            range: (RANGE_DB, -RANGE_DB),
                            switched,
                        }));
                    }
                    context.notify();
                }),
            );

        let controls = div()
            .flex()
            .flex_none()
            .gap(px(KNOB_GAP))
            .h(px(KNOB_HEIGHT))
            .child(self.band_numbers(input, &values, selected, theme, context))
            .child(self.band_switches(&params[selected], values[selected], theme, context))
            .children(self.param_knob(
                effect,
                EQ_BANDS[selected].params[0],
                "Freq",
                format_hz,
                true,
                theme,
                context,
            ))
            .child(if values[selected].mode.has_gain() {
                self.param_knob(
                    effect,
                    EQ_BANDS[selected].params[1],
                    "Gain",
                    format_db,
                    false,
                    theme,
                    context,
                )
                .map_or_else(|| div().into_any_element(), |knob| knob.into_any_element())
            } else {
                div().flex_none().w(px(KNOB_WIDTH)).into_any_element()
            })
            .children(self.param_knob(
                effect,
                EQ_BANDS[selected].params[2],
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

    /// The bands' numbers in two rows, coloured while on: click to pick one.
    fn band_numbers(
        &self,
        input: &EditorInput,
        values: &[BandValues; BANDS],
        selected: usize,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let enabled = input.enabled();
        let numbers = (0..BANDS).map(|band| {
            let editor = input.editor.clone();
            let number = div()
                .text_size(px(size::SMALL))
                .child((band + 1).to_string());
            let button = Flat::new(("eq-band", band), NUMBER_SIZE)
                .width(NUMBER_SIZE)
                .selected(band == selected);
            let button = if values[band].on {
                button.color(band_color(theme, band, enabled))
            } else {
                button
            };
            button.show(number, theme, context, move |_, _| {
                if let Editor::Eq(eq) = &mut *editor.borrow_mut() {
                    eq.selected = band;
                }
            })
        });
        div()
            .flex()
            .flex_none()
            .flex_wrap()
            .content_start()
            .w(px(4. * NUMBER_SIZE + 3. * 2.))
            .gap(px(2.))
            .children(numbers)
    }

    /// The picked band's switch, and its shape, the next one on a click.
    fn band_switches(
        &self,
        params: &BandParams,
        value: BandValues,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let on = params[4].clone();
        let switch = Flat::new("eq-on", NUMBER_SIZE)
            .width(SHAPE_WIDTH)
            .selected(value.on)
            .show(
                div().text_size(px(size::SMALL)).child("On"),
                theme,
                context,
                move |this, _| {
                    let (before, after) = (on.get(), if on.get() >= 0.5 { 0. } else { 1. });
                    on.set(after);
                    this.state.commit_params(&[(on.id, before, after)]);
                },
            );
        let shape = params[3].clone();
        let shape_button = Flat::new("eq-shape", NUMBER_SIZE).width(SHAPE_WIDTH).show(
            div()
                .text_size(px(size::SMALL))
                .child(shape_name(value.mode)),
            theme,
            context,
            move |this, _| {
                let index = FilterMode::ALL
                    .iter()
                    .position(|mode| *mode == eq_mode(shape.get()))
                    .unwrap_or(0);
                let next = FilterMode::ALL[(index + 1) % FilterMode::ALL.len()];
                let before = shape.get();
                shape.set(eq_mode_value(next));
                this.state.commit_params(&[(shape.id, before, shape.get())]);
            },
        );
        div()
            .flex()
            .flex_none()
            .flex_col()
            .gap(px(2.))
            .child(switch)
            .child(shape_button)
    }
}

/// Switches on the first band that's off at `hz`, `db` up, as one undo
/// step: a bell, or a cut near either end. The band, if one was free.
fn add_point(
    app: &mut Tonique,
    params: &[BandParams; BANDS],
    now: &[BandValues; BANDS],
    hz: f32,
    db: f32,
) -> Option<usize> {
    let band = (0..BANDS).find(|band| !now[*band].on)?;
    let mode = if hz < LOW_CUT_BELOW {
        FilterMode::HighPass
    } else if hz > HIGH_CUT_ABOVE {
        FilterMode::LowPass
    } else {
        FilterMode::Peak
    };
    let [freq, gain, q, shape, on] = &params[band];
    let changes = [
        (freq, hz),
        (shape, eq_mode_value(mode)),
        (q, q.default),
        (gain, if mode.has_gain() { db } else { 0. }),
        (on, 1.),
    ]
    .map(|(param, value)| {
        let before = param.get();
        param.set(value);
        (param.id, before, param.get())
    });
    app.state.commit_params(&changes);
    Some(band)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_has_a_name() {
        let names: std::collections::HashSet<_> = FilterMode::ALL
            .iter()
            .map(|mode| shape_name(*mode))
            .collect();
        assert_eq!(names.len(), FilterMode::ALL.len());
    }

    #[test]
    fn points_sit_at_their_gain_or_resonance() {
        let band = |mode, gain, q| BandValues {
            hz: 1000.,
            gain,
            q,
            mode,
            on: true,
        };
        assert_eq!(band(FilterMode::Peak, 6., 1.).height(), 6.);
        let cut = band(FilterMode::HighPass, 6., 10.).height();
        assert!((cut - 20.).abs() < 1e-3, "{cut}");
    }
}
