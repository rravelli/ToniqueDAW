//! The footer, along the master's header: its output scope.

use crate::{
    core::{
        metrics::SCOPE_COLUMNS,
        state::{MASTER_TRACK_ID, ProjectState},
    },
    ui::{
        arrangement::track_headers::FOOTER_PADDING,
        theme::{ThemeExt, font_size},
    },
};
use egui::{Align2, Color32, FontId, Rect, Stroke, Ui, pos2, vec2};
use std::collections::VecDeque;

use super::SCOPE_WIDTH;

/// The footer's background, and the master's output scope at its end, next
/// to the master's header.
pub(super) fn master_lane_ui(ui: &Ui, state: &ProjectState, footer: Rect) {
    let theme = ui.app_theme();
    let painter = ui.painter_at(footer);
    painter.rect_filled(footer, 0., theme.bg_panel);
    painter.hline(
        footer.x_range(),
        footer.top() + 0.5,
        Stroke::new(1.0, theme.border),
    );
    let lane = footer;

    let scope = Rect::from_min_max(
        pos2(
            lane.right() - SCOPE_WIDTH - 4.,
            lane.top() + FOOTER_PADDING + 2.,
        ),
        pos2(lane.right() - 4., lane.bottom() - 4.),
    );
    if scope.width() <= 0. || scope.height() <= 0. || lane.width() < 2. * SCOPE_WIDTH {
        return;
    }
    painter.rect_filled(scope, 2., theme.bg_deep);
    painter.hline(
        scope.x_range(),
        scope.center().y,
        Stroke::new(1.0, theme.separator),
    );
    if let Some(m) = state.metrics.tracks.get(&MASTER_TRACK_ID) {
        paint_scope(&painter, m.scope(), scope, theme.accent);
    }
    // Over the waveform, on a veil.
    let label = painter.text(
        scope.left_top() + vec2(4., 2.),
        Align2::LEFT_TOP,
        "OUT",
        FontId::proportional(font_size::SMALL),
        theme.text_muted,
    );
    painter.rect_filled(label.expand(1.), 1., theme.shadow);
    painter.text(
        label.min,
        Align2::LEFT_TOP,
        "OUT",
        FontId::proportional(font_size::SMALL),
        theme.text_muted,
    );
}

/// The master's output over the last few seconds, in `rect`, the newest at
/// the right: for each pixel, a bar from the lowest to the highest sample.
fn paint_scope(painter: &egui::Painter, columns: &VecDeque<[f32; 2]>, rect: Rect, color: Color32) {
    let width = rect.width().floor().max(1.) as usize;
    // The columns not yet filled are at the left.
    let missing = SCOPE_COLUMNS - columns.len().min(SCOPE_COLUMNS);
    let half = rect.height() / 2.;
    let y = |v: f32| rect.center().y - v.clamp(-1., 1.) * half;
    let mut mesh = egui::Mesh::default();
    for px in 0..width {
        let span = |px: usize| px * SCOPE_COLUMNS / width;
        let (from, to) = (span(px).max(missing), span(px + 1).max(span(px) + 1));
        let range = (from..to.min(SCOPE_COLUMNS))
            .map(|c| columns[c - missing])
            .reduce(|a, b| [a[0].min(b[0]), a[1].max(b[1])]);
        let Some([low, high]) = range else {
            continue;
        };
        let x = rect.left() + px as f32;
        // At least a pixel tall, so silence shows as a line.
        let (top, bottom) = (
            y(high).min(rect.center().y - 0.5),
            y(low).max(rect.center().y + 0.5),
        );
        mesh.add_colored_rect(Rect::from_x_y_ranges(x..=x + 1., top..=bottom), color);
    }
    painter.add(mesh);
}
