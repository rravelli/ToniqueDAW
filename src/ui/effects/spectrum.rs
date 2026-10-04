//! Editor of [`EffectKind::Spectrum`](crate::core::effect::EffectKind::Spectrum):
//! the spectrum of the audio where it sits in the chain, with a readout of
//! the frequency and level under the pointer.

use crate::ui::{
    effects::{
        EditorContext, EffectEditor, format_hz,
        graph::{level_y, paint_curve, paint_freq_grid, spectrum_points, x_freq},
    },
    theme::ThemeExt,
};
use egui::{Align2, FontId, Sense, Stroke, Ui, pos2, vec2};
use std::ops::RangeInclusive;
use tonique_engine::spectrum::Spectrum;

/// Levels shown, in dBFS.
const RANGE: RangeInclusive<f32> = -96.0..=0.;
/// Between level lines, in dB.
const LEVEL_STEP: f32 = 12.;

pub struct SpectrumEditor {
    spectrum: Spectrum,
}

impl SpectrumEditor {
    pub fn new() -> Self {
        Self {
            spectrum: Spectrum::new(),
        }
    }
}

impl EffectEditor for SpectrumEditor {
    fn ui(&mut self, ui: &mut Ui, cx: &mut EditorContext) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        let theme = ui.app_theme();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2., theme.bg_deep);
        paint_freq_grid(&painter, rect, &theme);
        let mut db = *RANGE.end() - LEVEL_STEP;
        while db > *RANGE.start() {
            let y = level_y(rect, &RANGE, db);
            painter.hline(rect.x_range(), y, Stroke::new(1., theme.grid_beat));
            painter.text(
                pos2(rect.right() - 2., y - 1.),
                Align2::RIGHT_BOTTOM,
                format!("{db:.0}"),
                FontId::proportional(8.),
                theme.text_muted,
            );
            db -= LEVEL_STEP;
        }

        if !cx.enabled() {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Bypassed",
                FontId::proportional(10.),
                theme.text_muted,
            );
            return;
        }
        let tap = &cx.effect.plugin.tap;
        // Keep it recording for half a second past the last frame shown.
        tap.watch_output_for((cx.sample_rate / 2.) as u32);
        self.spectrum
            .update(&tap.output, cx.sample_rate, ui.input(|i| i.stable_dt));
        let points = spectrum_points(&self.spectrum, rect, &RANGE);
        paint_curve(
            &painter,
            points,
            rect.bottom(),
            Stroke::new(1.5, theme.accent),
            theme.accent.gamma_multiply(0.15),
        );

        if let Some(pointer) = response.hover_pos() {
            let hz = x_freq(rect, pointer.x);
            let db = self.spectrum.level(hz);
            painter.vline(pointer.x, rect.y_range(), Stroke::new(1., theme.text_muted));
            painter.circle_filled(pos2(pointer.x, level_y(rect, &RANGE, db)), 3., theme.text);
            painter.text(
                rect.left_top() + vec2(4., 3.),
                Align2::LEFT_TOP,
                format!("{}  {db:.1} dB", format_hz(hz)),
                FontId::proportional(9.),
                theme.text,
            );
        }
    }

    fn width(&self) -> f32 {
        320.
    }
}
