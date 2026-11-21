//! Drawing the panel's graphs on canvases: lines, curves filled below,
//! circles and text placed by a corner or its middle.

use gpui::{Font, PathBuilder, Window, point, px};

use crate::{
    geometry::Rect,
    theme::{Color, FONT_UI, hsla},
    widgets::{paint_line, shape},
};

/// Which point of a text is placed: across (0 left, 0.5 middle, 1 right)
/// and down (0 top, 0.5 middle, 1 bottom).
#[derive(Clone, Copy)]
pub struct Align(pub f32, pub f32);

impl Align {
    pub const LEFT_TOP: Self = Self(0., 0.);
    pub const LEFT_BOTTOM: Self = Self(0., 1.);
    pub const CENTER_TOP: Self = Self(0.5, 0.);
    pub const CENTER: Self = Self(0.5, 0.5);
    pub const CENTER_BOTTOM: Self = Self(0.5, 1.);
    pub const RIGHT_TOP: Self = Self(1., 0.);
    pub const RIGHT_BOTTOM: Self = Self(1., 1.);
}

/// `text` with its `align` point at `(x, y)`, in the UI font unless
/// `font` says otherwise. Returns its width.
#[allow(clippy::too_many_arguments)]
pub fn text(
    window: &mut Window,
    context: &mut gpui::App,
    text: impl Into<gpui::SharedString>,
    (x, y): (f32, f32),
    align: Align,
    size: f32,
    color: Color,
    font: Option<Font>,
) -> f32 {
    let text = text.into();
    if text.is_empty() {
        return 0.;
    }
    let font = font.unwrap_or_else(|| gpui::font(FONT_UI));
    let line = shape(window, text, size, hsla(color), font);
    let width = f32::from(line.width);
    let height = size * 1.25;
    paint_line(
        window,
        context,
        &line,
        x - width * align.0,
        y - height * align.1,
        size,
    );
    width
}

pub fn fill(window: &mut Window, rect: Rect, color: Color) {
    window.paint_quad(gpui::fill(rect.bounds(), hsla(color)));
}

pub fn fill_rounded(window: &mut Window, rect: Rect, radius: f32, color: Color) {
    window.paint_quad(gpui::fill(rect.bounds(), hsla(color)).corner_radii(px(radius)));
}

/// A line through `points`, `width` thick.
pub fn polyline(window: &mut Window, points: &[(f32, f32)], width: f32, color: Color) {
    let Some(&(first_x, first_y)) = points.first() else {
        return;
    };
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(point(px(first_x), px(first_y)));
    for &(x, y) in &points[1..] {
        path.line_to(point(px(x), px(y)));
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, hsla(color));
    }
}

/// A filled polygon through `points`.
pub fn polygon(window: &mut Window, points: &[(f32, f32)], color: Color) {
    let Some(&(first_x, first_y)) = points.first() else {
        return;
    };
    let mut path = PathBuilder::fill();
    path.move_to(point(px(first_x), px(first_y)));
    for &(x, y) in &points[1..] {
        path.line_to(point(px(x), px(y)));
    }
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, hsla(color));
    }
}

/// A curve going left to right, filled faintly down to `bottom`.
pub fn curve(
    window: &mut Window,
    points: &[(f32, f32)],
    bottom: f32,
    (line, width): (Color, f32),
    below: Color,
) {
    if points.len() < 2 {
        return;
    }
    let mut area: Vec<(f32, f32)> = points.iter().map(|&(x, y)| (x, y.min(bottom))).collect();
    let (first, last) = (points[0].0, points[points.len() - 1].0);
    area.push((last, bottom));
    area.push((first, bottom));
    polygon(window, &area, below);
    polyline(window, points, width, line);
}

/// A circle around `(x, y)`: filled, and outlined when `outline` is.
pub fn circle(
    window: &mut Window,
    (x, y): (f32, f32),
    radius: f32,
    fill: Color,
    outline: Option<(f32, Color)>,
) {
    let rect = Rect::new(x - radius, y - radius, 2. * radius, 2. * radius);
    let quad = gpui::fill(rect.bounds(), hsla(fill)).corner_radii(px(radius));
    let quad = match outline {
        Some((width, color)) => quad.border_widths(px(width)).border_color(hsla(color)),
        None => quad,
    };
    window.paint_quad(quad);
}
