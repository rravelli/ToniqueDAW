//! Text painted on a canvas.

use gpui::{Font, Hsla, ShapedLine, SharedString, TextRun, Window, point, px};

/// `text` shaped on one line, in `font` at `size`.
pub fn shape(
    window: &mut Window,
    text: impl Into<SharedString>,
    size: f32,
    color: Hsla,
    font: Font,
) -> ShapedLine {
    let text = text.into();
    let run = TextRun {
        len: text.len(),
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(text, px(size), &[run], None)
}

/// Paints a shaped line with its top left at `(left, top)`, `size` its font's.
pub fn paint_line(
    window: &mut Window,
    context: &mut gpui::App,
    line: &ShapedLine,
    left: f32,
    top: f32,
    size: f32,
) {
    line.paint(
        point(px(left), px(top)),
        px(size * 1.25),
        gpui::TextAlign::Left,
        None,
        window,
        context,
    )
    .ok();
}

/// One line of text, its top left at `(left, top)` in window coordinates.
/// Returns its width.
#[allow(clippy::too_many_arguments)]
pub fn paint_text(
    window: &mut Window,
    context: &mut gpui::App,
    text: impl Into<SharedString>,
    left: f32,
    top: f32,
    size: f32,
    color: Hsla,
    font: Font,
) -> f32 {
    let text = text.into();
    if text.is_empty() {
        return 0.;
    }
    let line = shape(window, text, size, color, font);
    paint_line(window, context, &line, left, top, size);
    f32::from(line.width)
}
