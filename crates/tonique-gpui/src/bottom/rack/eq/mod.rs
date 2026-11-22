//! The EQ's editor, as the egui app's: the bands' summed curve, coloured
//! by the bands shaping it, over the live spectrum; a point per band
//! switched on, dragged for its frequency and gain (or resonance), added
//! and removed by double-clicks, right-clicked for its shape. Below, the
//! picked band's number (its switch), shape and knobs; in a full window, a
//! card per band. The graph is in [`graph`], the controls in
//! [`controls`].

mod controls;
mod graph;

use std::{cell::Cell, rc::Rc};

use gpui::{
    Anchor, AnyElement, Bounds, ContentMask, Context, CursorStyle, MouseButton, MouseDownEvent,
    Pixels, Stateful, canvas, div, prelude::*, px,
};
use tonique_engine::{
    dsp::Biquad,
    edit::{Parameter, PluginId, TrackId},
    nodes::{EQ_BANDS, EQ_GAIN_DB, FilterMode, eq_mode, eq_mode_value},
};

use crate::{
    app::{PopupKind, Tonique},
    bottom::{
        paint::{Align, circle, fill, fill_rounded, polyline, text},
        rack::{
            RackDrag,
            editors::{Editor, EditorInput, KNOB_GAP},
            filter::{GraphDrag, LiveSpectrum, Scale, from_height},
            format_db, format_hz,
            graph::{hline, paint_freq_grid},
            knob::{KNOB_HEIGHT, KNOB_WIDTH},
            shapes::{SHAPE_BUTTON, shape_menu, shape_row},
        },
    },
    core::effect::Effect,
    geometry::Rect,
    icons,
    theme::{Color, FONT_UI, Theme, faded, hsla, mix, size},
    widgets::{Menu, MenuItem, select},
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
const BAND_WIDTH: f32 = 110.;
const ROW_HEIGHT: f32 = 20.;
const NUMBER_WIDTH: f32 = 24.;
/// A band's card of controls in a full window, and the space inside it.
const CARD_PADDING: f32 = 6.;
const CARD_WIDTH: f32 = 3. * KNOB_WIDTH + 2. * KNOB_GAP + 2. * CARD_PADDING;
const CARD_HEIGHT: f32 = 2. * CARD_PADDING + ROW_HEIGHT + 4. + SHAPE_BUTTON.1 + 4. + KNOB_HEIGHT;
const CARD_GAP: f32 = 8.;
/// Graphs at least this tall label their gain lines.
const LABELLED_HEIGHT: f32 = 160.;
/// How far a band's colour reaches along the curve, in octaves.
const COLOR_REACH: f32 = 0.9;
/// How much the curve keeps a neutral colour where no band is near.
const NEUTRAL_WEIGHT: f32 = 0.06;
/// The curve's line is drawn in runs this many pixels long, each in the
/// colour at its middle.
const COLOR_RUN: usize = 3;
const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

/// Shapes in the order offered: low to high, roughly.
const SHAPES: [FilterMode; 7] = [
    FilterMode::HighPass,
    FilterMode::LowShelf,
    FilterMode::Peak,
    FilterMode::Notch,
    FilterMode::BandPass,
    FilterMode::HighShelf,
    FilterMode::LowPass,
];

/// What a band's shape is called.
pub fn shape_name(mode: FilterMode) -> &'static str {
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

    /// Where its point is up the graph, in dB: its gain, or if its shape
    /// has none, its resonance (Q, as a cut's gain at its cutoff).
    fn height(&self) -> f32 {
        let value = if self.mode.has_gain() {
            self.gain
        } else {
            self.q
        };
        from_height(self.mode, value)
    }

    fn biquad(&self, sample_rate: f32) -> Biquad {
        Biquad::new(self.mode, self.hz, self.q, self.gain, sample_rate)
    }

    /// How much it shapes the curve at `hz`, where it changes it by `db`:
    /// near its frequency, and all along past it for a cut or shelf, more
    /// the more it changes there.
    fn weight(&self, hz: f32, db: f32) -> f32 {
        let octaves = (hz / self.hz).log2();
        // A cut owns all it cuts; a shelf reaches along its side less
        // strongly, so a band inside it shows.
        let beyond = match self.mode {
            FilterMode::HighPass if octaves < 0. => 1.,
            FilterMode::LowPass if octaves > 0. => 1.,
            FilterMode::LowShelf if octaves < 0. => 0.6,
            FilterMode::HighShelf if octaves > 0. => 0.6,
            _ => 0.,
        };
        let near = (-0.5 * (octaves / COLOR_REACH).powi(2)).exp();
        near.max(beyond) * (0.2 + (db.abs() / 3.).min(4.))
    }
}

/// What the drawn curve depends on: the bands, the sample rate, the
/// width, whether it's enabled, and the colours.
type CurveKey = ([BandValues; BANDS], f32, f32, bool, Vec<Color>);

/// The curve worked out for a key: a gain and a colour per pixel.
#[derive(Default)]
struct Curve {
    key: Option<CurveKey>,
    db: Vec<f32>,
    colors: Vec<Color>,
}

/// What the EQ's editor keeps.
pub struct EqState {
    pub spectrum: LiveSpectrum,
    /// The band whose controls show.
    selected: usize,
    /// The graph, as last laid out.
    graph: Rc<Cell<Bounds<Pixels>>>,
    curve: Curve,
}

impl EqState {
    pub fn new() -> Self {
        Self {
            spectrum: LiveSpectrum::new(),
            // A bell in the middle.
            selected: 3,
            graph: Rc::default(),
            curve: Curve::default(),
        }
    }

    /// The summed response and its colours across `scale`, worked out
    /// again only when they change.
    fn curve(
        &mut self,
        bands: [BandValues; BANDS],
        sample_rate: f32,
        scale: &Scale,
        theme: &Theme,
        enabled: bool,
    ) -> &Curve {
        let mut colors = theme.palette.clone();
        colors.extend([theme.accent, theme.text_muted, theme.text_disabled]);
        let key = (bands, sample_rate, scale.rect.width, enabled, colors);
        if self.curve.key.as_ref() != Some(&key) {
            let on: Vec<(usize, Biquad)> = (0..BANDS)
                .filter(|band| bands[*band].on)
                .map(|band| (band, bands[band].biquad(sample_rate)))
                .collect();
            let rect = scale.rect;
            let (mut db, mut colors) = (Vec::new(), Vec::new());
            for column in 0..=rect.width.ceil() as usize {
                let hz = scale.hz(rect.left + column as f32);
                let mut total = 0.;
                // A running weighted average of the colours.
                let mut color = theme.text_muted;
                let mut weight = NEUTRAL_WEIGHT;
                for (band, filter) in &on {
                    let band_db = filter.response_db(hz, sample_rate);
                    total += band_db;
                    let band_weight = bands[*band].weight(hz, band_db);
                    weight += band_weight;
                    color = mix(color, band_color(theme, *band, true), band_weight / weight);
                }
                db.push(total);
                colors.push(if enabled { color } else { theme.text_disabled });
            }
            self.curve = Curve {
                key: Some(key),
                db,
                colors,
            };
        }
        &self.curve
    }
}

/// Each band's own colour: spread over the theme's palette so no two
/// bands share one.
pub fn band_color(theme: &Theme, band: usize, enabled: bool) -> Color {
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

/// The band whose point is nearest `(x, y)`, within reach; only bands
/// switched on have a point, and the one picked.
fn nearest(
    scale: &Scale,
    values: &[BandValues; BANDS],
    selected: usize,
    (x, y): (f32, f32),
) -> Option<usize> {
    (0..BANDS)
        .filter(|band| values[*band].on || *band == selected)
        .map(|band| {
            let point = (scale.x(values[band].hz), scale.y(values[band].height()));
            (band, (point.0 - x).hypot(point.1 - y))
        })
        .filter(|(_, distance)| *distance <= GRAB_RADIUS)
        .min_by(|first, second| first.1.total_cmp(&second.1))
        .map(|(band, _)| band)
}

/// Sets `changes` now, as one undo step.
fn commit(app: &mut Tonique, changes: &[(&Parameter, f32)]) {
    let changes: Vec<_> = changes
        .iter()
        .map(|(param, value)| {
            let before = param.get();
            param.set(*value);
            (param.id, before, param.get())
        })
        .filter(|(_, before, after)| before != after)
        .collect();
    if !changes.is_empty() {
        app.state.commit_params(&changes);
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
        let (selected, tap) = match &*input.editor.borrow() {
            Editor::Eq(eq) => (eq.selected, eq.spectrum.tap),
            _ => return div().into_any_element(),
        };
        let values = params.each_ref().map(BandValues::of);
        let graph = self.eq_graph(input, &params, values, selected, theme, context);
        let graph = div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(graph)
            .when(input.enabled(), |graph| {
                graph.child(self.tap_buttons(effect.plugin.id, tap, theme, context))
            });
        let controls = if input.expanded {
            self.eq_cards(input, &params, values, selected, theme, context)
                .into_any_element()
        } else {
            self.eq_controls(input, &params, values[selected], selected, theme, context)
                .into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(if input.expanded { 8. } else { 4. }))
            .child(graph)
            .child(controls)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_is_offered_once() {
        let offered: std::collections::HashSet<_> = SHAPES.iter().collect();
        assert_eq!(offered.len(), FilterMode::ALL.len());
        let names: std::collections::HashSet<_> =
            SHAPES.iter().map(|mode| shape_name(*mode)).collect();
        assert_eq!(names.len(), SHAPES.len());
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

    /// A band colours the curve near it, and all along past a cut.
    #[test]
    fn a_band_weighs_near_it_and_past_a_cut() {
        let bell = BandValues {
            hz: 1000.,
            gain: 6.,
            q: 1.,
            mode: FilterMode::Peak,
            on: true,
        };
        assert!(bell.weight(1000., 6.) > bell.weight(8000., 6.));
        let cut = BandValues {
            mode: FilterMode::HighPass,
            ..bell
        };
        assert!(cut.weight(30., -24.) > 1., "owns what it cuts");
    }
}
