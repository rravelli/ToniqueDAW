//! Editor of [`EffectKind::Filter`](crate::core::effect::EffectKind::Filter):
//! its response over the live spectrum, with a handle for cutoff and
//! resonance, the mode, and cutoff and Q knobs.

use crate::{
    core::effect::Setting,
    ui::{
        effects::{
            EditorContext, EffectEdit, EffectEditor, format_hz,
            graph::{freq_x, paint_curve, paint_freq_grid, spectrum_points, x_freq},
            graph_rect, track_drag,
        },
        theme::{Theme, ThemeExt},
        widget::flat_button::FlatButton,
    },
};
use egui::{
    Align, Align2, CursorIcon, FontId, Layout, Painter, Pos2, Rect, Sense, Stroke, Ui, UiBuilder,
    pos2, vec2,
};
use std::{f32::consts::PI, ops::RangeInclusive};
use tonique_engine::{edit::PluginKind, nodes::FilterMode, spectrum::Spectrum};

/// Gain range of the graph, in dB.
const TOP_DB: f32 = 24.;
const BOTTOM_DB: f32 = -36.;
/// Levels of the spectrum shown, in dBFS.
const SPECTRUM_RANGE: RangeInclusive<f32> = -90.0..=0.;
const HANDLE_RADIUS: f32 = 5.;

/// Which side of the filter the spectrum shows.
#[derive(Clone, Copy, PartialEq)]
enum Tap {
    Input,
    Output,
}

pub struct FilterEditor {
    spectrum: Spectrum,
    tap: Tap,
    /// The response drawn last, a gain per pixel, and what it was for:
    /// worked out again only when that changes.
    response: (Option<ResponseKey>, Vec<f32>),
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

impl FilterEditor {
    pub fn new() -> Self {
        Self {
            spectrum: Spectrum::new(),
            tap: Tap::Input,
            response: (None, Vec::new()),
        }
    }

    /// The filter's gain at each pixel across `scale`, in dB.
    fn response(&mut self, key: ResponseKey, scale: &Scale) -> &[f32] {
        if self.response.0 != Some(key) {
            let rect = scale.rect;
            self.response = (
                Some(key),
                (0..=key.width.ceil() as usize)
                    .map(|i| {
                        let hz = scale.hz(rect.left() + i as f32);
                        response_db(key.mode, key.cutoff, key.q, key.sample_rate, hz)
                    })
                    .collect(),
            );
        }
        &self.response.1
    }

    /// The audio going into the filter, or coming out.
    fn paint_spectrum(
        &mut self,
        ui: &mut Ui,
        painter: &Painter,
        scale: &Scale,
        cx: &EditorContext,
        theme: &Theme,
    ) {
        let tap = &cx.effect.plugin.tap;
        // Keep it recording for half a second past the last frame shown.
        let frames = (cx.sample_rate / 2.) as u32;
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
        self.spectrum
            .update(meter, cx.sample_rate, ui.input(|i| i.stable_dt));

        let rect = scale.rect;
        let points = spectrum_points(&self.spectrum, rect, &SPECTRUM_RANGE);
        paint_curve(
            painter,
            points,
            rect.bottom(),
            Stroke::new(1., theme.text_muted),
            theme.text_disabled.gamma_multiply(0.3),
        );
    }

    /// Buttons picking which side of the filter the spectrum shows.
    fn tap_buttons(&mut self, ui: &mut Ui, rect: Rect) {
        // Over the graph, out of the layout: `put` would move the controls
        // below them.
        let ui = &mut ui.new_child(UiBuilder::new().max_rect(rect));
        let mut at = rect.left_top() + vec2(3., 3.);
        for (tap, name, tooltip) in [
            (Tap::Input, "Pre", "Show the audio going into the filter"),
            (
                Tap::Output,
                "Post",
                "Show the audio coming out of the filter",
            ),
        ] {
            let button = FlatButton::new(name)
                .size(vec2(26., 14.))
                .font(FontId::proportional(9.))
                .selected(self.tap == tap)
                .tooltip(tooltip);
            if ui
                .put(Rect::from_min_size(at, vec2(26., 14.)), button)
                .clicked()
            {
                self.tap = tap;
            }
            at.x += 28.;
        }
    }
}

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
        freq_x(self.rect, hz)
    }
    fn hz(&self, x: f32) -> f32 {
        x_freq(self.rect, x)
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
            self.paint_spectrum(ui, &painter, &scale, cx, &theme);
        }
        let (cutoff, q) = (cutoff.get(), q.get());
        let color = if cx.enabled() {
            theme.accent
        } else {
            theme.text_disabled
        };
        let key = ResponseKey {
            mode,
            cutoff,
            q,
            sample_rate: cx.sample_rate,
            width: rect.width(),
        };
        let points: Vec<Pos2> = self
            .response(key, &scale)
            .iter()
            .enumerate()
            .map(|(i, db)| pos2(rect.left() + i as f32, scale.y(*db)))
            .collect();
        paint_curve(
            &painter,
            points,
            rect.bottom(),
            Stroke::new(1.5, color),
            color.gamma_multiply(0.15),
        );

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

        if cx.enabled() {
            self.tap_buttons(ui, rect);
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

/// The frequency grid; lines every 12 dB, stronger at 0 dB.
fn paint_grid(painter: &Painter, scale: &Scale, theme: &Theme) {
    let rect = scale.rect;
    paint_freq_grid(painter, rect, theme);
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
        assert_eq!(scale.x(super::super::graph::MIN_HZ), 10.);
        assert_eq!(scale.y(TOP_DB), 20.);
    }
}
