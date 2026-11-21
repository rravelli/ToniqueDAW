//! Drawing shared by editors with a frequency axis: the axis, its grid,
//! and spectrum curves.

use std::ops::RangeInclusive;

use gpui::Window;
use tonique_engine::spectrum::Spectrum;

use crate::{
    bottom::{
        paint::{Align, fill, text},
        rack::format_hz,
    },
    geometry::Rect,
    theme::{Theme, size},
};

pub const MIN_HZ: f32 = 20.;
pub const MAX_HZ: f32 = 20_000.;

/// Where `hz` is across `rect`, on a log scale from [`MIN_HZ`] to
/// [`MAX_HZ`].
pub fn freq_x(rect: Rect, hz: f32) -> f32 {
    rect.left + (hz / MIN_HZ).ln() / (MAX_HZ / MIN_HZ).ln() * rect.width
}

/// The frequency at `x` across `rect`.
pub fn x_freq(rect: Rect, x: f32) -> f32 {
    MIN_HZ * (MAX_HZ / MIN_HZ).powf((x - rect.left) / rect.width)
}

/// Where `db` is down `rect`, `range` being the levels shown (quietest
/// first), clamped to it.
pub fn level_y(rect: Rect, range: &RangeInclusive<f32>, db: f32) -> f32 {
    let (bottom, top) = (*range.start(), *range.end());
    rect.bottom() - ((db - bottom) / (top - bottom)).clamp(0., 1.) * rect.height
}

/// Lines at 1-2-5 steps, labelled at the decades.
pub fn paint_freq_grid(window: &mut Window, context: &mut gpui::App, rect: Rect, theme: &Theme) {
    let mut decade = 10.;
    while decade < MAX_HZ {
        for step in [1., 2., 5.] {
            let hz = decade * step;
            if !(MIN_HZ..=MAX_HZ).contains(&hz) {
                continue;
            }
            let x = freq_x(rect, hz).round();
            let color = if step == 1. {
                theme.grid_bar
            } else {
                theme.grid_beat
            };
            fill(window, Rect::new(x, rect.top, 1., rect.height), color);
            if step == 1. {
                text(
                    window,
                    context,
                    format_hz(hz).replace(' ', ""),
                    (x + 2., rect.bottom() - 1.),
                    Align::LEFT_BOTTOM,
                    size::SMALL,
                    theme.text_muted,
                    None,
                );
            }
        }
        decade *= 10.;
    }
}

/// A level line across `rect` at `y`.
pub fn hline(window: &mut Window, rect: Rect, y: f32, color: crate::theme::Color) {
    fill(
        window,
        Rect::new(rect.left, y.round(), rect.width, 1.),
        color,
    );
}

/// `spectrum` as a point per pixel across `rect`, `range` being the
/// levels shown in dBFS.
pub fn spectrum_points(
    spectrum: &Spectrum,
    rect: Rect,
    range: &RangeInclusive<f32>,
) -> Vec<(f32, f32)> {
    (0..=rect.width.ceil() as usize)
        .map(|column| {
            let x = rect.left + column as f32;
            let db = spectrum.level(x_freq(rect, x));
            (x, level_y(rect, range, db))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes_map_back() {
        let rect = Rect::new(10., 20., 300., 100.);
        assert!((x_freq(rect, freq_x(rect, 1234.)) - 1234.).abs() < 0.1);
        assert_eq!(freq_x(rect, MIN_HZ), 10.);
        assert_eq!(level_y(rect, &(-90.0..=0.), 0.), 20.);
        assert_eq!(level_y(rect, &(-90.0..=0.), -200.), 120.);
    }
}
