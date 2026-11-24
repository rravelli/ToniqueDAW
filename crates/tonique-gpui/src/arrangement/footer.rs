//! The timeline's footer, along the master's header in the track list:
//! the master's output scope at its end, as `master_lane_ui` in the egui
//! app.

use gpui::{Window, px};

use crate::{
    arrangement::FOOTER_PADDING,
    geometry::Rect,
    theme::{Color, FONT_UI, Theme, hsla, size},
    widgets::paint_text,
};
use tonique_core::core::metrics::SCOPE_COLUMNS;

/// The master's output scope.
const SCOPE_WIDTH: f32 = 180.;
/// Between the scope and the footer's edges.
const SCOPE_MARGIN: f32 = 4.;
const LABEL: &str = "OUT";

/// The footer over `footer`, and the scope of `columns` (the master's
/// output, oldest first) at its end.
pub fn paint_footer(
    window: &mut Window,
    context: &mut gpui::App,
    footer: Rect,
    columns: &[[f32; 2]],
    theme: &Theme,
) {
    let fill = |window: &mut Window, rect: Rect, color: Color| {
        window.paint_quad(gpui::fill(rect.bounds(), hsla(color)))
    };
    fill(window, footer, theme.bg_panel);
    fill(
        window,
        Rect::new(footer.left, footer.top, footer.width, 1.),
        theme.border,
    );
    let top = footer.top + FOOTER_PADDING + 2.;
    let scope = Rect::new(
        footer.right() - SCOPE_WIDTH - SCOPE_MARGIN,
        top,
        SCOPE_WIDTH,
        footer.bottom() - SCOPE_MARGIN - top,
    );
    // Only where there's room for it beside the lanes' content.
    if scope.is_empty() || footer.width < 2. * SCOPE_WIDTH {
        return;
    }
    window.paint_quad(gpui::fill(scope.bounds(), hsla(theme.bg_deep)).corner_radii(px(2.)));
    let middle = scope.top + scope.height / 2.;
    fill(
        window,
        Rect::new(scope.left, middle.round(), scope.width, 1.),
        theme.separator,
    );
    paint_scope(window, scope, columns, theme.accent);
    // Over the waveform, on a veil.
    let (label_left, label_top) = (scope.left + 4., scope.top + 2.);
    let veil = Rect::new(
        label_left - 1.,
        label_top - 1.,
        20.,
        size::SMALL * 1.25 + 2.,
    );
    window.paint_quad(gpui::fill(veil.bounds(), hsla(theme.shadow)).corner_radii(px(1.)));
    paint_text(
        window,
        context,
        LABEL,
        label_left,
        label_top,
        size::SMALL,
        hsla(theme.text_muted),
        gpui::font(FONT_UI),
    );
}

/// The master's output over the last few seconds in `rect`, the newest at
/// the right: for each pixel, a bar from the lowest to the highest sample.
fn paint_scope(window: &mut Window, rect: Rect, columns: &[[f32; 2]], color: Color) {
    let width = rect.width.floor().max(1.) as usize;
    // The columns not filled yet are at the left.
    let missing = SCOPE_COLUMNS - columns.len().min(SCOPE_COLUMNS);
    let middle = rect.top + rect.height / 2.;
    let half = rect.height / 2.;
    let height_of = |sample: f32| middle - sample.clamp(-1., 1.) * half;
    let span = |pixel: usize| pixel * SCOPE_COLUMNS / width;
    for pixel in 0..width {
        let from = span(pixel).max(missing);
        let to = span(pixel + 1).max(span(pixel) + 1).min(SCOPE_COLUMNS);
        let range = (from..to)
            .filter_map(|column| columns.get(column - missing))
            .copied()
            .reduce(|first, second| [first[0].min(second[0]), first[1].max(second[1])]);
        let Some([low, high]) = range else {
            continue;
        };
        // At least a pixel tall, so silence shows as a line.
        let top = height_of(high).min(middle - 0.5);
        let bottom = height_of(low).max(middle + 0.5);
        let bar = Rect::new(rect.left + pixel as f32, top, 1., bottom - top);
        window.paint_quad(gpui::fill(bar.bounds(), hsla(color)));
    }
}
