//! Drawing shared by editors with a frequency axis: the axis, its grid,
//! and spectrum curves.

use crate::ui::{effects::format_hz, theme::Theme};
use egui::{Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, pos2};
use std::ops::RangeInclusive;
use tonique_engine::spectrum::Spectrum;

pub const MIN_HZ: f32 = 20.;
pub const MAX_HZ: f32 = 20_000.;

/// Where `hz` is across `rect`, on a log scale from [`MIN_HZ`] to
/// [`MAX_HZ`].
pub fn freq_x(rect: Rect, hz: f32) -> f32 {
    rect.left() + (hz / MIN_HZ).ln() / (MAX_HZ / MIN_HZ).ln() * rect.width()
}

/// The frequency at `x` across `rect`.
pub fn x_freq(rect: Rect, x: f32) -> f32 {
    MIN_HZ * (MAX_HZ / MIN_HZ).powf((x - rect.left()) / rect.width())
}

/// Where `db` is down `rect`, `range` being the levels shown (quietest
/// first), clamped to it.
pub fn level_y(rect: Rect, range: &RangeInclusive<f32>, db: f32) -> f32 {
    let (bottom, top) = (*range.start(), *range.end());
    rect.bottom() - ((db - bottom) / (top - bottom)).clamp(0., 1.) * rect.height()
}

/// Lines at 1-2-5 steps, labelled at the decades.
pub fn paint_freq_grid(painter: &Painter, rect: Rect, theme: &Theme) {
    let mut decade = 10.;
    while decade < MAX_HZ {
        for step in [1., 2., 5.] {
            let hz = decade * step;
            if !(MIN_HZ..=MAX_HZ).contains(&hz) {
                continue;
            }
            let x = freq_x(rect, hz);
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
}

/// `spectrum` as a point per pixel across `rect`, `range` being the
/// levels shown in dBFS.
pub fn spectrum_points(spectrum: &Spectrum, rect: Rect, range: &RangeInclusive<f32>) -> Vec<Pos2> {
    (0..=rect.width().ceil() as usize)
        .map(|i| {
            let x = rect.left() + i as f32;
            let db = spectrum.level(x_freq(rect, x));
            pos2(x, level_y(rect, range, db))
        })
        .collect()
}

/// A curve, filled below with a faint `fill`.
pub fn paint_curve(painter: &Painter, points: Vec<Pos2>, bottom: f32, line: Stroke, fill: Color32) {
    painter.add(fill_below(&points, bottom, fill));
    painter.add(Shape::line(points, line));
}

/// Fill between a curve going left to right and `bottom`. Unlike a
/// polygon fill, it can be any shape.
pub fn fill_below(points: &[Pos2], bottom: f32, color: Color32) -> Shape {
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
    use egui::vec2;

    #[test]
    fn axes_map_back() {
        let rect = Rect::from_min_size(pos2(10., 20.), vec2(300., 100.));
        assert!((x_freq(rect, freq_x(rect, 1234.)) - 1234.).abs() < 0.1);
        assert_eq!(freq_x(rect, MIN_HZ), 10.);
        assert_eq!(level_y(rect, &(-90.0..=0.), 0.), 20.);
        assert_eq!(level_y(rect, &(-90.0..=0.), -200.), 120.);
    }
}
