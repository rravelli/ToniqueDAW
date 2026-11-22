//! A plain horizontal slider's look: a track filled in the accent up to a
//! round knob.

use gpui::{Window, px};

use crate::{
    geometry::Rect,
    theme::{Theme, hsla},
};

/// A horizontal slider at `value` (0 to 1) over `rect`.
pub fn paint_slider(window: &mut Window, rect: Rect, value: f32, theme: &Theme) {
    let value = value.clamp(0., 1.);
    let middle = rect.top + rect.height / 2.;
    let track = Rect::new(rect.left, middle - 2., rect.width, 4.);
    window.paint_quad(gpui::fill(track.bounds(), hsla(theme.bg_control)).corner_radii(px(2.)));
    window.paint_quad(
        gpui::fill(
            Rect::new(rect.left, middle - 2., rect.width * value, 4.).bounds(),
            hsla(theme.accent),
        )
        .corner_radii(px(2.)),
    );
    let knob = Rect::new(rect.left + rect.width * value - 5., middle - 5., 10., 10.);
    window.paint_quad(gpui::fill(knob.bounds(), hsla(theme.text)).corner_radii(px(5.)));
}
