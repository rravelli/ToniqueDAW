//! Editor of [`EffectKind::Eq`](crate::core::effect::EffectKind::Eq): the
//! summed response of its bands over the live spectrum, with a point per
//! band switched on, each band in a colour of its own. The curve takes
//! the colour of the bands shaping it, blending between them. Double-click
//! the graph to add a point, a point to remove it; drag a point to move
//! it; right-click it to change its shape. Below, the controls of the band
//! picked; in a full window, of every band.

use crate::ui::theme::font_size;
use crate::ui::{
    effects::{
        EditorContext, EffectEdit, EffectEditor, format_db, format_hz,
        graph::{FreqScale, LiveSpectrum},
        graph_rect,
        shapes::{ShapePicker, paint_shape_icon, shape_row},
    },
    font::semibold,
    theme::{Theme, ThemeExt},
    widget::{
        context_menu::{ContextMenuButton, ContextMenuLabel, ContextMenuSeparator},
        flat_button::FlatButton,
        knob::Knob,
        select::Select,
    },
};
use egui::{
    Align, Align2, Color32, CursorIcon, FontId, Layout, Mesh, Pos2, Rect, Response, ScrollArea,
    Sense, Stroke, Ui, UiBuilder, pos2, vec2,
};
use egui_phosphor::fill::TRASH;
use tonique_engine::{
    dsp::Biquad,
    edit::Parameter,
    nodes::{EQ_BANDS, EQ_GAIN_DB, FilterMode, eq_mode, eq_mode_value},
};

const BANDS: usize = EQ_BANDS.len();
/// Gain range of the graph, in dB.
const RANGE_DB: f32 = EQ_GAIN_DB + 6.;
const POINT_RADIUS: f32 = 6.5;
/// How near a point a press must be to grab it, in points.
const GRAB_RADIUS: f32 = 10.;
/// A new point at the graph's ends is a cut, below and above these.
const LOW_CUT_BELOW: f32 = 40.;
const HIGH_CUT_ABOVE: f32 = 15_000.;
const BAND_WIDTH: f32 = 110.;
const ROW_HEIGHT: f32 = 20.;
const NUMBER_WIDTH: f32 = 24.;
/// A band's card of controls in a full window, and the space inside it.
const CARD_PADDING: f32 = 6.;
const CARD_WIDTH: f32 = 3. * Knob::WIDTH + 2. * 6. + 2. * CARD_PADDING;
const CARD_HEIGHT: f32 = 2. * CARD_PADDING + ROW_HEIGHT + 4. + ROW_HEIGHT + 4. + Knob::HEIGHT;
const CARD_GAP: f32 = 8.;
/// Graphs at least this tall label their gain lines.
const LABELLED_HEIGHT: f32 = 160.;
/// How far a band's colour reaches along the curve, in octaves.
const COLOR_REACH: f32 = 0.9;
/// How much the curve keeps a neutral colour where no band is near.
const NEUTRAL_WEIGHT: f32 = 0.06;

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
    /// Where its point is up the graph, in dB: its gain, or if its shape
    /// has none, its resonance (Q, as a cut's gain at its cutoff).
    fn height(&self) -> f32 {
        if self.mode.has_gain() {
            self.gain
        } else {
            20. * self.q.max(1e-3).log10()
        }
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

/// A band's parameters: frequency, gain, Q, shape, on.
type BandParams = [Parameter; 5];

/// What an EQ's drawn curve depends on: its bands, the sample rate, the
/// width, and whether it's enabled (its colours).
type CurveKey = ([BandValues; BANDS], f32, f32, bool);

/// The curve worked out for a key: a gain and a colour per pixel.
struct Curve {
    key: Option<CurveKey>,
    db: Vec<f32>,
    colors: Vec<Color32>,
}

/// What a point's drag started from, for its undo step.
struct Drag {
    band: usize,
    /// Frequency, height (gain or Q) and the switch.
    before: [f32; 3],
}

pub struct EqEditor {
    spectrum: LiveSpectrum,
    selected: usize,
    drag: Option<Drag>,
    /// The point whose menu is open.
    menu: Option<usize>,
    curve: Curve,
}

impl EqEditor {
    pub fn new() -> Self {
        Self {
            spectrum: LiveSpectrum::new(),
            // A bell in the middle.
            selected: 3,
            drag: None,
            menu: None,
            curve: Curve {
                key: None,
                db: Vec::new(),
                colors: Vec::new(),
            },
        }
    }

    /// The summed response and its colours across `scale`, worked out
    /// again only when they change.
    fn curve(
        &mut self,
        bands: [BandValues; BANDS],
        sample_rate: f32,
        scale: &FreqScale,
        theme: &Theme,
        enabled: bool,
    ) -> &Curve {
        let key = (bands, sample_rate, scale.rect.width(), enabled);
        if self.curve.key != Some(key) {
            let on: Vec<(usize, Biquad)> = (0..BANDS)
                .filter(|b| bands[*b].on)
                .map(|b| (b, bands[b].biquad(sample_rate)))
                .collect();
            let rect = scale.rect;
            let (mut db, mut colors) = (Vec::new(), Vec::new());
            for i in 0..=rect.width().ceil() as usize {
                let hz = scale.hz(rect.left() + i as f32);
                let mut total = 0.;
                // A running weighted average of the colours.
                let mut color = theme.text_muted;
                let mut weight = NEUTRAL_WEIGHT;
                for (b, k) in &on {
                    let band_db = k.response_db(hz, sample_rate);
                    total += band_db;
                    let w = bands[*b].weight(hz, band_db);
                    weight += w;
                    color = color.lerp_to_gamma(band_color(theme, *b, true), w / weight);
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

    /// The graph: drag, add, remove and pick points, and draw it all.
    fn graph(
        &mut self,
        ui: &mut Ui,
        cx: &mut EditorContext,
        params: &[BandParams; BANDS],
        rect: Rect,
        response: &Response,
    ) {
        let scale = FreqScale {
            rect,
            top_db: RANGE_DB,
            bottom_db: -RANGE_DB,
        };
        let before = values(params);
        let point = |v: &BandValues| pos2(scale.x(v.hz), scale.y(v.height()));
        // Only switched-on bands have a point, and the one picked.
        let selected = self.selected;
        let shown = |b: usize| before[b].on || b == selected;
        let nearest = |pos: Pos2| {
            (0..BANDS)
                .filter(|b| shown(*b))
                .map(|b| (b, point(&before[b]).distance(pos)))
                .filter(|(_, d)| *d <= GRAB_RADIUS)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(b, _)| b)
        };
        let hovered = response.hover_pos().and_then(nearest);
        let pressed = response.interact_pointer_pos().and_then(nearest);

        // Drag a point to move it: frequency across, gain (or resonance)
        // up. Dragging one that's off switches it on.
        if response.drag_started()
            && let Some(band) = pressed
        {
            self.selected = band;
            let [hz, .., on] = &params[band];
            self.drag = Some(Drag {
                band,
                before: [hz.get(), height_param(params, band).get(), on.get()],
            });
            on.set(1.);
        }
        if response.clicked()
            && let Some(band) = pressed
        {
            self.selected = band;
        }
        if let Some(drag) = &self.drag {
            let band = drag.band;
            let [hz, .., on] = &params[band];
            if let Some(pos) = response.interact_pointer_pos() {
                hz.set(scale.hz(pos.x));
                let db = scale.db(pos.y);
                height_param(params, band).set(if before[band].mode.has_gain() {
                    db
                } else {
                    10f32.powf(db / 20.)
                });
            }
            if response.drag_stopped() {
                let changed = [hz, height_param(params, band), on]
                    .into_iter()
                    .zip(drag.before);
                for (p, old) in changed {
                    push_edit(cx, p, old);
                }
                self.drag = None;
            }
        }

        if response.double_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            match nearest(pos) {
                // Remove it: switch it off.
                Some(band) => set(cx, &params[band][4], 0.),
                None => self.add_point(cx, params, &before, scale.hz(pos.x), scale.db(pos.y)),
            }
        }

        // Right-click a point for its shape.
        if response.secondary_clicked() {
            self.menu = pressed;
            if let Some(band) = pressed {
                self.selected = band;
            }
        }
        if let Some(band) = self.menu {
            response.context_menu(|ui| {
                let theme = ui.app_theme();
                let color = band_color(&theme, band, true);
                ui.add(ContextMenuLabel::new(format!("Band {}", band + 1)));
                for mode in SHAPES {
                    let on = before[band].mode == mode;
                    let item = ui.add(ContextMenuButton::new("", shape_name(mode)).checked(on));
                    // In the icon's place.
                    let icon = Rect::from_min_size(item.rect.min + vec2(2., 3.), vec2(13., 10.));
                    let ink = if on { color } else { theme.text_muted };
                    paint_shape_icon(ui.painter(), icon, mode, Stroke::new(1.2, ink));
                    if item.clicked() {
                        set(cx, &params[band][3], eq_mode_value(mode));
                        set(cx, &params[band][4], 1.);
                    }
                }
                ui.add(ContextMenuSeparator::new());
                let remove = ContextMenuButton::new(TRASH, "Remove").text_color(theme.danger);
                if ui.add(remove).clicked() {
                    set(cx, &params[band][4], 0.);
                }
            });
        }

        if hovered.is_some() || self.drag.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }

        let theme = ui.app_theme();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3., theme.bg_deep);
        scale.paint_grid(&painter, 6., &theme);
        // Labelled when there's room: small, they'd sit under the points.
        let labels: &[f32] = if rect.height() > LABELLED_HEIGHT {
            &[-12., -6., 6., 12.]
        } else {
            &[]
        };
        for &db in labels {
            painter.text(
                pos2(rect.left() + 4., scale.y(db) - 1.),
                Align2::LEFT_BOTTOM,
                format!("{db:+.0}"),
                FontId::proportional(font_size::SMALL),
                theme.text_muted,
            );
        }
        if cx.enabled() {
            let tap = cx.effect.plugin.tap.clone();
            self.spectrum
                .paint(ui, &painter, rect, &tap, cx.sample_rate, &theme);
        }
        let now = values(params);
        let enabled = cx.enabled();

        // The band picked on its own, dashed, in its colour, where it
        // changes anything.
        let picked = now[self.selected];
        if picked.on {
            let k = picked.biquad(cx.sample_rate);
            let tint = band_color(&theme, self.selected, enabled);
            let mut run: Vec<Pos2> = Vec::new();
            for i in 0..=rect.width().ceil() as usize + 1 {
                let x = rect.left() + i as f32;
                let db = k.response_db(scale.hz(x), cx.sample_rate);
                if db.abs() > 0.1 && x <= rect.right() + 1. {
                    run.push(pos2(x, scale.y(db)));
                } else if run.len() > 1 {
                    painter.extend(egui::Shape::dashed_line(
                        &std::mem::take(&mut run),
                        Stroke::new(1., tint.gamma_multiply(0.7)),
                        4.,
                        3.,
                    ));
                } else {
                    run.clear();
                }
            }
        }

        let zero = scale.y(0.);
        let curve = self.curve(now, cx.sample_rate, &scale, &theme, enabled);
        let points: Vec<Pos2> = curve
            .db
            .iter()
            .enumerate()
            .map(|(i, db)| pos2(rect.left() + i as f32, scale.y(*db)))
            .collect();
        painter.add(gradient_fill(&points, &curve.colors, zero));
        painter.add(gradient_line(&points, &curve.colors, 2.));

        let dragged = self.drag.as_ref().map(|d| d.band);
        for (b, v) in now.iter().enumerate() {
            if !(v.on || b == self.selected) {
                continue;
            }
            paint_point(
                &painter,
                &theme,
                point(v),
                b,
                v.on && enabled,
                b == self.selected,
                hovered == Some(b) || dragged == Some(b),
            );
        }
        if let Some(b) = hovered.or(dragged) {
            let v = now[b];
            let detail = if v.mode.has_gain() {
                format_db(v.gain)
            } else {
                format!("Q {:.2}", v.q)
            };
            painter.text(
                rect.right_top() + vec2(-6., 4.),
                Align2::RIGHT_TOP,
                format!("{} · {} · {detail}", shape_name(v.mode), format_hz(v.hz)),
                FontId::proportional(font_size::LABEL),
                band_color(&theme, b, enabled),
            );
        } else if !now.iter().any(|v| v.on) {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Double-click to add a point",
                FontId::proportional(font_size::LABEL),
                theme.text_muted,
            );
        }
    }

    /// Switch on the first band that's off, at `hz` and `db`: a cut at the
    /// graph's ends, a bell between.
    fn add_point(
        &mut self,
        cx: &mut EditorContext,
        params: &[BandParams; BANDS],
        now: &[BandValues; BANDS],
        hz: f32,
        db: f32,
    ) {
        let Some(band) = (0..BANDS).find(|b| !now[*b].on) else {
            return;
        };
        let mode = if hz < LOW_CUT_BELOW {
            FilterMode::HighPass
        } else if hz > HIGH_CUT_ABOVE {
            FilterMode::LowPass
        } else {
            FilterMode::Peak
        };
        let [freq, gain, q, shape, on] = &params[band];
        set(cx, freq, hz);
        set(cx, shape, eq_mode_value(mode));
        set(cx, q, q.default);
        set(cx, gain, if mode.has_gain() { db } else { 0. });
        set(cx, on, 1.);
        self.selected = band;
    }

    /// The band's number, filled with its colour while it's on, outlined
    /// in it while off: click to switch it.
    fn number(
        &mut self,
        ui: &mut Ui,
        cx: &mut EditorContext,
        params: &[BandParams; BANDS],
        band: usize,
    ) {
        let theme = ui.app_theme();
        let on = params[band][4].get() >= 0.5;
        let color = band_color(&theme, band, cx.enabled());
        let button = FlatButton::new((band + 1).to_string())
            .size(vec2(NUMBER_WIDTH, ROW_HEIGHT))
            .border_radius(ROW_HEIGHT / 2.)
            .font(semibold(font_size::LABEL))
            .tooltip(if on {
                "Switch the band off"
            } else {
                "Switch the band on"
            });
        let button = if on {
            button.fill(color).color(theme.bg_deep)
        } else {
            button.stroke(Stroke::new(1., color)).color(color)
        };
        if ui.add(button).clicked() {
            set(cx, &params[band][4], if on { 0. } else { 1. });
            self.selected = band;
        }
    }

    /// Freq, Gain (in its place, if its shape has one) and Q knobs.
    fn knobs(ui: &mut Ui, cx: &mut EditorContext, band: usize, mode: FilterMode) {
        let [freq, gain, q, ..] = EQ_BANDS[band].params;
        cx.param_knob(ui, freq, "Freq", &format_hz, true);
        if mode.has_gain() {
            cx.param_knob(ui, gain, "Gain", &format_db, false);
        } else {
            ui.add_space(Knob::WIDTH);
        }
        cx.param_knob(ui, q, "Q", &|q| format!("{q:.2}"), true);
    }

    /// In a full window: a card per band, side by side, with its number
    /// and shape's name, a button per shape, and its knobs. The band
    /// picked is outlined in its colour.
    fn all_bands(&mut self, ui: &mut Ui, cx: &mut EditorContext, params: &[BandParams; BANDS]) {
        let theme = ui.app_theme();
        let now = values(params);
        ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = CARD_GAP;
                for (band, v) in now.iter().enumerate() {
                    let (card, _) =
                        ui.allocate_exact_size(vec2(CARD_WIDTH, CARD_HEIGHT), Sense::hover());
                    let color = band_color(&theme, band, v.on && cx.enabled());
                    let picked = band == self.selected;
                    let (fill, outline) = if picked {
                        (theme.bg_control_hover, Stroke::new(1.5, color))
                    } else {
                        (theme.bg_control, Stroke::new(1., theme.border))
                    };
                    ui.painter()
                        .rect(card, 6., fill, outline, egui::StrokeKind::Inside);
                    let mut inner = ui.new_child(
                        UiBuilder::new()
                            .id_salt(("eq-band-card", band))
                            .max_rect(card.shrink(CARD_PADDING))
                            .layout(Layout::top_down(Align::Min)),
                    );
                    inner.spacing_mut().item_spacing = vec2(6., 4.);
                    inner.horizontal(|ui| {
                        self.number(ui, cx, params, band);
                        let ink = if v.on { theme.text } else { theme.text_muted };
                        ui.label(
                            egui::RichText::new(shape_name(v.mode))
                                .font(semibold(font_size::LABEL))
                                .color(ink),
                        );
                    });
                    if let Some(mode) = shape_row(&mut inner, &SHAPES, v.mode, color, shape_name) {
                        set(cx, &params[band][3], eq_mode_value(mode));
                        set(cx, &params[band][4], 1.);
                        self.selected = band;
                    }
                    inner.horizontal(|ui| Self::knobs(ui, cx, band, v.mode));
                    if ui.rect_contains_pointer(card) && ui.input(|i| i.pointer.primary_pressed()) {
                        self.selected = band;
                    }
                }
            });
        });
    }
}

/// A band's point: filled with its colour when on, an outline when off;
/// larger and haloed under the pointer, ringed when picked.
fn paint_point(
    painter: &egui::Painter,
    theme: &Theme,
    at: Pos2,
    band: usize,
    on: bool,
    picked: bool,
    hot: bool,
) {
    let color = if on {
        band_color(theme, band, true)
    } else {
        theme.text_disabled
    };
    let radius = if hot || picked {
        POINT_RADIUS + 1.5
    } else {
        POINT_RADIUS
    };
    if hot {
        painter.circle_filled(at, radius + 5., color.gamma_multiply(0.2));
    }
    if on {
        painter.circle_filled(at, radius, color);
    } else {
        painter.circle(at, radius, theme.bg_deep, Stroke::new(1.5, color));
    }
    if picked {
        painter.circle_stroke(at, radius + 2., Stroke::new(1.5, theme.text));
    }
    painter.text(
        at,
        Align2::CENTER_CENTER,
        (band + 1).to_string(),
        semibold(font_size::SMALL),
        if on { theme.bg_deep } else { color },
    );
}

/// The parameters of each band.
fn band_params(cx: &EditorContext) -> Option<[BandParams; BANDS]> {
    let mut found = Vec::with_capacity(BANDS);
    for band in &EQ_BANDS {
        let [a, b, c, d, e] = band.params.map(|name| cx.param(name).cloned());
        found.push([a?, b?, c?, d?, e?]);
    }
    found.try_into().ok()
}

fn values(params: &[BandParams; BANDS]) -> [BandValues; BANDS] {
    std::array::from_fn(|b| {
        let [hz, gain, q, shape, on] = &params[b];
        BandValues {
            hz: hz.get(),
            gain: gain.get(),
            q: q.get(),
            mode: eq_mode(shape.get()),
            on: on.get() >= 0.5,
        }
    })
}

/// The parameter a band's point sets with its height.
fn height_param(params: &[BandParams; BANDS], band: usize) -> &Parameter {
    let [_, gain, q, shape, _] = &params[band];
    if eq_mode(shape.get()).has_gain() {
        gain
    } else {
        q
    }
}

/// Each band's own colour: spread over the theme's palette so no two
/// bands share one.
pub fn band_color(theme: &Theme, band: usize, enabled: bool) -> Color32 {
    if !enabled {
        return theme.text_disabled;
    }
    match theme.palette.len() {
        0 => theme.accent,
        n => theme.palette[band * n / BANDS % n],
    }
}

/// Set `param` to `new` now, as part of the editor's undo step.
fn set(cx: &mut EditorContext, param: &Parameter, new: f32) {
    let old = param.get();
    param.set(new);
    push_edit(cx, param, old);
}

/// Record `param`'s change from `old`, if it changed.
fn push_edit(cx: &mut EditorContext, param: &Parameter, old: f32) {
    let new = param.get();
    if new != old {
        cx.edits.push(EffectEdit::Param {
            id: param.id,
            old,
            new,
        });
    }
}

/// A line through `points`, each in its colour, the colours blending
/// between them.
fn gradient_line(points: &[Pos2], colors: &[Color32], width: f32) -> egui::Shape {
    let mut mesh = Mesh::default();
    let half = width / 2.;
    for (i, (p, c)) in points.iter().zip(colors).enumerate() {
        let prev = points[i.saturating_sub(1)];
        let next = points[(i + 1).min(points.len() - 1)];
        let dir = (next - prev).normalized();
        let normal = vec2(-dir.y, dir.x) * half;
        mesh.colored_vertex(*p + normal, *c);
        mesh.colored_vertex(*p - normal, *c);
        if i > 0 {
            let n = 2 * i as u32;
            mesh.add_triangle(n - 2, n - 1, n);
            mesh.add_triangle(n - 1, n, n + 1);
        }
    }
    egui::Shape::mesh(mesh)
}

/// Fill between a curve and the line at `y`, on either side of it, in the
/// curve's colours: strongest at the curve, fading towards the line.
fn gradient_fill(points: &[Pos2], colors: &[Color32], y: f32) -> egui::Shape {
    let mut mesh = Mesh::default();
    for (i, (p, c)) in points.iter().zip(colors).enumerate() {
        mesh.colored_vertex(*p, c.gamma_multiply(0.3));
        mesh.colored_vertex(pos2(p.x, y), c.gamma_multiply(0.04));
        if i > 0 {
            let n = 2 * i as u32;
            mesh.add_triangle(n - 2, n - 1, n);
            mesh.add_triangle(n - 1, n, n + 1);
        }
    }
    egui::Shape::mesh(mesh)
}

impl EffectEditor for EqEditor {
    fn ui(&mut self, ui: &mut Ui, cx: &mut EditorContext) {
        let Some(params) = band_params(cx) else {
            return;
        };
        let (rect, response) = if cx.expanded {
            // Room for the bands' cards below.
            let below = CARD_HEIGHT + 24.;
            let size = vec2(
                ui.available_width(),
                (ui.available_height() - below).max(0.),
            );
            ui.allocate_exact_size(size, Sense::click_and_drag())
        } else {
            graph_rect(ui, Sense::click_and_drag())
        };
        self.graph(ui, cx, &params, rect, &response);
        if cx.enabled() {
            self.spectrum.buttons(ui, rect, "EQ");
        }

        if cx.expanded {
            ui.add_space(8.);
            self.all_bands(ui, cx, &params);
            return;
        }
        let band = self.selected;
        let v = values(&params)[band];
        let color = band_color(&ui.app_theme(), band, v.on && cx.enabled());
        ui.horizontal(|ui| {
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                let mut picked = self.selected;
                let select = Select::new(("eq-band", cx.effect.plugin.id.0), &mut picked)
                    .options((0..BANDS).map(|b| (b, format!("Band {}", b + 1))))
                    .width(BAND_WIDTH);
                if ui.add(select).changed() {
                    self.selected = picked;
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.;
                    self.number(ui, cx, &params, band);
                    let mut mode = v.mode;
                    let picker = ShapePicker::new(
                        ("eq-shape", cx.effect.plugin.id.0, band),
                        &mut mode,
                        &SHAPES,
                        &shape_name,
                    )
                    .width(BAND_WIDTH - NUMBER_WIDTH - 4.)
                    .color(color);
                    if ui.add(picker).changed() {
                        set(cx, &params[band][3], eq_mode_value(mode));
                        set(cx, &params[band][4], 1.);
                    }
                });
            });
            Self::knobs(ui, cx, band, v.mode);
        });
    }

    fn width(&self) -> f32 {
        380.
    }

    fn expandable(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_is_offered_once() {
        for mode in FilterMode::ALL {
            assert_eq!(SHAPES.iter().filter(|m| **m == mode).count(), 1);
        }
    }

    #[test]
    fn bands_have_colours_of_their_own() {
        let theme = Theme::dark();
        let colors: Vec<_> = (0..BANDS).map(|b| band_color(&theme, b, true)).collect();
        for (i, a) in colors.iter().enumerate() {
            assert!(
                !colors[i + 1..].contains(a),
                "band {} shares a colour",
                i + 1
            );
        }
    }

    #[test]
    fn a_band_colours_the_curve_near_it_and_past_a_cut() {
        let band = |mode| BandValues {
            hz: 1000.,
            gain: 6.,
            q: 1.,
            mode,
            on: true,
        };
        let bell = band(FilterMode::Peak);
        assert!(bell.weight(1000., 6.) > bell.weight(4000., 0.5) * 10.);
        let cut = band(FilterMode::HighPass);
        assert!(cut.weight(50., -30.) > cut.weight(4000., 0.) * 10.);
    }
}
