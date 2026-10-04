//! Editor of [`EffectKind::Filter`](crate::core::effect::EffectKind::Filter):
//! its response over the live spectrum, with a handle for cutoff and
//! resonance, the mode, and cutoff and Q knobs.

use crate::{
    core::effect::Setting,
    ui::{
        effects::{EditorContext, EffectEdit, EffectEditor, format_hz, graph_rect, track_drag},
        theme::{Theme, ThemeExt},
        widget::flat_button::FlatButton,
    },
};
use egui::{
    Align, Align2, Color32, CursorIcon, FontId, Layout, Mesh, Painter, Pos2, Rect, Sense, Shape,
    Stroke, Ui, pos2, vec2,
};
use std::f32::consts::PI;
use tonique_engine::{edit::PluginKind, nodes::FilterMode};

const MIN_HZ: f32 = 20.;
const MAX_HZ: f32 = 20_000.;
/// Gain range of the graph, in dB.
const TOP_DB: f32 = 24.;
const BOTTOM_DB: f32 = -36.;
/// Quietest level of the spectrum shown, in dBFS.
const SPECTRUM_FLOOR: f32 = -96.;
const HANDLE_RADIUS: f32 = 5.;

pub struct FilterEditor;

/// Gain of the engine's filter at `hz`, in dB. Mirrors
/// [`FilterNode`](tonique_engine::nodes::FilterNode)'s RBJ biquad, so the
/// curve shows what's heard.
pub fn response_db(mode: FilterMode, cutoff: f32, q: f32, sample_rate: f32, hz: f32) -> f32 {
    let f = cutoff.clamp(10., sample_rate * 0.49);
    let w0 = 2. * PI * f / sample_rate;
    let (sin0, cos0) = w0.sin_cos();
    let alpha = sin0 / (2. * q.max(0.05));
    let (b0, b1, b2) = match mode {
        FilterMode::LowPass => ((1. - cos0) / 2., 1. - cos0, (1. - cos0) / 2.),
        FilterMode::HighPass => ((1. + cos0) / 2., -(1. + cos0), (1. + cos0) / 2.),
    };
    let (a0, a1, a2) = (1. + alpha, -2. * cos0, 1. - alpha);
    // H(e^jw), with z^-1 = e^-jw.
    let w = 2. * PI * hz / sample_rate;
    let magnitude = |c0: f32, c1: f32, c2: f32| {
        let re = c0 + c1 * w.cos() + c2 * (2. * w).cos();
        let im = -(c1 * w.sin() + c2 * (2. * w).sin());
        (re * re + im * im).sqrt()
    };
    20. * (magnitude(b0, b1, b2) / magnitude(a0, a1, a2))
        .max(1e-9)
        .log10()
}

/// Maps frequencies and gains to the graph and back.
struct Scale {
    rect: Rect,
}

impl Scale {
    fn x(&self, hz: f32) -> f32 {
        self.rect.left() + (hz / MIN_HZ).ln() / (MAX_HZ / MIN_HZ).ln() * self.rect.width()
    }
    fn hz(&self, x: f32) -> f32 {
        MIN_HZ * (MAX_HZ / MIN_HZ).powf((x - self.rect.left()) / self.rect.width())
    }
    fn y(&self, db: f32) -> f32 {
        self.rect.top() + (TOP_DB - db) / (TOP_DB - BOTTOM_DB) * self.rect.height()
    }
    fn db(&self, y: f32) -> f32 {
        TOP_DB - (y - self.rect.top()) / self.rect.height() * (TOP_DB - BOTTOM_DB)
    }
}

impl EffectEditor for FilterEditor {
    fn ui(&mut self, ui: &mut Ui, cx: &mut EditorContext) {
        let PluginKind::Filter(mode) = cx.effect.plugin.kind else {
            return;
        };
        let (Some(cutoff), Some(q)) = (cx.param("cutoff").cloned(), cx.param("q").cloned()) else {
            return;
        };
        let (rect, response) = graph_rect(ui, Sense::click_and_drag());
        let scale = Scale { rect };

        // Drag anywhere: the handle follows, setting the cutoff from x and
        // the resonance (the gain at the cutoff, which is Q) from y.
        let before = (cutoff.get(), q.get());
        if response.dragged()
            && let Some(pos) = response.interact_pointer_pos()
        {
            cutoff.set(scale.hz(pos.x));
            q.set(10f32.powf(scale.db(pos.y) / 20.));
        }
        if response.double_clicked() {
            for p in [&cutoff, &q] {
                if let Some(value) = cx.effect.kind.initial_value(&cx.effect.plugin, p.name) {
                    p.set(value);
                }
            }
        }
        for (p, before) in [(&cutoff, before.0), (&q, before.1)] {
            if let Some(old) = track_drag(ui, &response, before) {
                cx.edits.push(EffectEdit::Param {
                    id: p.id,
                    old,
                    new: p.get(),
                });
            }
        }
        let active = response.hovered() || response.dragged();
        if active {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }

        let theme = ui.app_theme();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2., theme.bg_deep);
        paint_grid(&painter, &scale, &theme);
        if cx.enabled() {
            paint_spectrum(&painter, &scale, cx, &theme);
        }
        let (cutoff, q) = (cutoff.get(), q.get());
        let color = if cx.enabled() {
            theme.accent
        } else {
            theme.text_disabled
        };
        let points: Vec<Pos2> = (0..=rect.width().ceil() as usize)
            .map(|i| {
                let x = rect.left() + i as f32;
                let db = response_db(mode, cutoff, q, cx.sample_rate, scale.hz(x));
                pos2(x, scale.y(db))
            })
            .collect();
        painter.add(fill_below(
            &points,
            rect.bottom(),
            color.gamma_multiply(0.15),
        ));
        painter.add(Shape::line(points, Stroke::new(1.5, color)));

        let handle = pos2(scale.x(cutoff), scale.y(20. * q.log10()));
        painter.circle(
            handle,
            HANDLE_RADIUS,
            if active { color } else { theme.bg_deep },
            Stroke::new(1.5, color),
        );
        if active {
            painter.text(
                rect.right_top() + vec2(-4., 3.),
                Align2::RIGHT_TOP,
                format!("{} · Q {q:.2}", format_hz(cutoff)),
                FontId::proportional(9.),
                theme.text,
            );
        }

        ui.horizontal(|ui| {
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 2.;
                for (option, name, tooltip) in [
                    (FilterMode::LowPass, "LP", "Low-pass: cut above the cutoff"),
                    (
                        FilterMode::HighPass,
                        "HP",
                        "High-pass: cut below the cutoff",
                    ),
                ] {
                    let button = FlatButton::new(name)
                        .size(vec2(28., 18.))
                        .font(FontId::proportional(10.))
                        .selected(mode == option)
                        .tooltip(tooltip);
                    if ui.add(button).clicked() && mode != option {
                        cx.edits.push(EffectEdit::Setting(Setting::Mode(option)));
                    }
                }
            });
            cx.param_knob(ui, "cutoff", "Freq", &format_hz, true);
            cx.param_knob(ui, "q", "Q", &|q| format!("{q:.2}"), true);
        });
    }

    fn width(&self) -> f32 {
        280.
    }
}

/// Lines at 1-2-5 steps, labelled at the decades; the 0 dB line.
fn paint_grid(painter: &Painter, scale: &Scale, theme: &Theme) {
    let rect = scale.rect;
    let mut decade = 10.;
    while decade < MAX_HZ {
        for step in [1., 2., 5.] {
            let hz = decade * step;
            if !(MIN_HZ..=MAX_HZ).contains(&hz) {
                continue;
            }
            let x = scale.x(hz);
            let color = if step == 1. {
                theme.grid_bar
            } else {
                theme.grid_beat
            };
            painter.vline(x, rect.y_range(), Stroke::new(1., color));
            if step == 1. {
                painter.text(
                    pos2(x + 2., rect.bottom() - 2.),
                    Align2::LEFT_BOTTOM,
                    format_hz(hz).replace(' ', ""),
                    FontId::proportional(8.),
                    theme.text_muted,
                );
            }
        }
        decade *= 10.;
    }
    let mut db = (BOTTOM_DB / 12.).ceil() * 12.;
    while db < TOP_DB {
        let color = if db == 0. {
            theme.grid_bar
        } else {
            theme.grid_beat
        };
        painter.hline(rect.x_range(), scale.y(db), Stroke::new(1., color));
        db += 12.;
    }
}

/// The track's spectrum, its loudest bin per pixel column.
fn paint_spectrum(painter: &Painter, scale: &Scale, cx: &EditorContext, theme: &Theme) {
    let spectrum = cx.metrics.spectrum();
    if spectrum.is_empty() {
        return;
    }
    let rect = scale.rect;
    let bin_hz = cx.sample_rate / (2 * spectrum.len()) as f32;
    let y = |db: f32| {
        rect.bottom() - ((db - SPECTRUM_FLOOR) / -SPECTRUM_FLOOR).clamp(0., 1.) * rect.height()
    };
    let mut points: Vec<Pos2> = Vec::new();
    for (i, db) in spectrum.iter().enumerate().skip(1) {
        let x = scale.x(i as f32 * bin_hz).round();
        if x < rect.left() || x > rect.right() {
            continue;
        }
        match points.last_mut() {
            Some(last) if last.x == x => last.y = last.y.min(y(*db)),
            _ => points.push(pos2(x, y(*db))),
        }
    }
    painter.add(fill_below(
        &points,
        rect.bottom(),
        theme.text_disabled.gamma_multiply(0.25),
    ));
    painter.add(Shape::line(points, Stroke::new(1., theme.text_disabled)));
}

/// Fill between a curve going left to right and `bottom`. Unlike a
/// polygon fill, it can be any shape.
fn fill_below(points: &[Pos2], bottom: f32, color: Color32) -> Shape {
    let mut mesh = Mesh::default();
    for (i, p) in points.iter().enumerate() {
        mesh.colored_vertex(*p, color);
        mesh.colored_vertex(pos2(p.x, bottom.max(p.y)), color);
        if i > 0 {
            let n = 2 * i as u32;
            mesh.add_triangle(n - 2, n - 1, n);
            mesh.add_triangle(n - 1, n, n + 1);
        }
    }
    Shape::mesh(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.;

    #[test]
    fn response_matches_the_filter() {
        for mode in [FilterMode::LowPass, FilterMode::HighPass] {
            // The gain at the cutoff is Q.
            let db = response_db(mode, 1000., 4., SR, 1000.);
            assert!((db - 20. * 4f32.log10()).abs() < 0.01, "{mode:?}: {db}");
        }
        let low = |hz| response_db(FilterMode::LowPass, 1000., 0.707, SR, hz);
        assert!(low(20.).abs() < 0.01, "passes the lows");
        assert!(low(10_000.) < -35., "cuts the highs: {}", low(10_000.));
        let high = |hz| response_db(FilterMode::HighPass, 1000., 0.707, SR, hz);
        assert!(high(15_000.).abs() < 0.1, "passes the highs");
        assert!(high(100.) < -35., "cuts the lows: {}", high(100.));
    }

    #[test]
    fn scale_maps_back() {
        let scale = Scale {
            rect: Rect::from_min_size(pos2(10., 20.), vec2(300., 100.)),
        };
        assert!((scale.hz(scale.x(1234.)) - 1234.).abs() < 0.1);
        assert!((scale.db(scale.y(-7.)) + 7.).abs() < 1e-3);
        assert_eq!(scale.x(MIN_HZ), 10.);
        assert_eq!(scale.y(TOP_DB), 20.);
    }
}
