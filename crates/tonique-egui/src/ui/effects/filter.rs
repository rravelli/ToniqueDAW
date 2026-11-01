//! Editor of [`EffectKind::Filter`](crate::core::effect::EffectKind::Filter):
//! its response over the live spectrum, with a handle for the frequency
//! and resonance (or gain, for the shelves and peak), the mode, and
//! frequency, Q and gain knobs.

use crate::ui::theme::font_size;
use crate::{
    core::effect::Setting,
    ui::{
        effects::{
            EditorContext, EffectEdit, EffectEditor, format_db, format_hz,
            graph::{FreqScale, LiveSpectrum, paint_curve},
            graph_rect,
            shapes::ShapePicker,
            track_drag,
        },
        theme::ThemeExt,
    },
};
use egui::{Align, Align2, CursorIcon, FontId, Layout, Pos2, Sense, Stroke, Ui, pos2, vec2};
use tonique_engine::{dsp::Biquad, edit::PluginKind, nodes::FilterMode};

/// Gain range of the graph, in dB.
const TOP_DB: f32 = 24.;
const BOTTOM_DB: f32 = -36.;
const HANDLE_RADIUS: f32 = 5.;
const MODE_WIDTH: f32 = 96.;

pub struct FilterEditor {
    spectrum: LiveSpectrum,
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
    gain: f32,
    sample_rate: f32,
    width: f32,
}

/// Shown in the mode's dropdown.
pub fn mode_name(mode: FilterMode) -> &'static str {
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

impl FilterEditor {
    pub fn new() -> Self {
        Self {
            spectrum: LiveSpectrum::new(),
            response: (None, Vec::new()),
        }
    }

    /// The filter's gain at each pixel across `scale`, in dB.
    fn response(&mut self, key: ResponseKey, scale: &FreqScale) -> &[f32] {
        if self.response.0 != Some(key) {
            let k = Biquad::new(key.mode, key.cutoff, key.q, key.gain, key.sample_rate);
            let rect = scale.rect;
            self.response = (
                Some(key),
                (0..=key.width.ceil() as usize)
                    .map(|i| k.response_db(scale.hz(rect.left() + i as f32), key.sample_rate))
                    .collect(),
            );
        }
        &self.response.1
    }
}

/// The value of the handle's parameter at `db` up the graph: the gain
/// itself, or Q.
fn to_height(mode: FilterMode, db: f32) -> f32 {
    if mode.has_gain() {
        db
    } else {
        10f32.powf(db / 20.)
    }
}

/// Where the handle's parameter puts it up the graph, in dB.
fn from_height(mode: FilterMode, value: f32) -> f32 {
    if mode.has_gain() {
        value
    } else {
        20. * value.max(1e-3).log10()
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
        let gain = cx.param("gain").cloned();
        let (rect, response) = graph_rect(ui, Sense::click_and_drag());
        let scale = FreqScale {
            rect,
            top_db: TOP_DB,
            bottom_db: BOTTOM_DB,
        };
        // The handle's height: the gain for the shelves and peak, else the
        // resonance (the gain at the cutoff of a low- or high-pass, which
        // is Q).
        let height = match &gain {
            Some(gain) if mode.has_gain() => gain,
            _ => &q,
        };

        // Drag anywhere: the handle follows, setting the frequency from x
        // and its height from y.
        let before = (cutoff.get(), height.get());
        if response.dragged()
            && let Some(pos) = response.interact_pointer_pos()
        {
            cutoff.set(scale.hz(pos.x));
            height.set(to_height(mode, scale.db(pos.y)));
        }
        if response.double_clicked() {
            for p in [&cutoff, height] {
                if let Some(value) = cx.effect.kind.initial_value(&cx.effect.plugin, p.name) {
                    p.set(value);
                }
            }
        }
        for (p, before) in [(&cutoff, before.0), (height, before.1)] {
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
        scale.paint_grid(&painter, 12., &theme);
        if cx.enabled() {
            let tap = cx.effect.plugin.tap.clone();
            self.spectrum
                .paint(ui, &painter, rect, &tap, cx.sample_rate, &theme);
        }
        let color = if cx.enabled() {
            theme.accent
        } else {
            theme.text_disabled
        };
        let key = ResponseKey {
            mode,
            cutoff: cutoff.get(),
            q: q.get(),
            gain: gain.as_ref().map_or(0., |g| g.get()),
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

        let handle = pos2(
            scale.x(key.cutoff),
            scale.y(from_height(mode, height.get())),
        );
        painter.circle(
            handle,
            HANDLE_RADIUS,
            if active { color } else { theme.bg_deep },
            Stroke::new(1.5, color),
        );
        if active {
            let detail = if mode.has_gain() {
                format_db(key.gain)
            } else {
                format!("Q {:.2}", key.q)
            };
            painter.text(
                rect.right_top() + vec2(-4., 3.),
                Align2::RIGHT_TOP,
                format!("{} · {detail}", format_hz(key.cutoff)),
                FontId::proportional(font_size::SMALL),
                theme.text,
            );
        }

        if cx.enabled() {
            self.spectrum.buttons(ui, rect, "filter");
        }
        ui.horizontal(|ui| {
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                let mut picked = mode;
                let theme = ui.app_theme();
                let color = if cx.enabled() {
                    theme.accent
                } else {
                    theme.text_disabled
                };
                let picker = ShapePicker::new(
                    ("filter-mode", cx.effect.plugin.id.0),
                    &mut picked,
                    &FilterMode::ALL,
                    &mode_name,
                )
                .width(MODE_WIDTH)
                .color(color);
                if ui.add(picker).changed() && picked != mode {
                    cx.edits.push(EffectEdit::Setting(Setting::Mode(picked)));
                }
            });
            cx.param_knob(ui, "cutoff", "Freq", &format_hz, true);
            cx.param_knob(ui, "q", "Q", &|q| format!("{q:.2}"), true);
            if mode.has_gain() {
                cx.param_knob(ui, "gain", "Gain", &format_db, false);
            }
        });
    }

    fn width(&self) -> f32 {
        300.
    }

    fn expandable(&self) -> bool {
        true
    }
}
