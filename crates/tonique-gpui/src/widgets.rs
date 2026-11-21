//! The few controls the views share: buttons, toggles, a slider's look, and
//! text painted on a canvas.

use gpui::{
    Context, Font, Hsla, MouseButton, SharedString, Stateful, TextRun, Window, div, point,
    prelude::*, px,
};

use crate::{
    app::Tonique,
    theme::{Color, Theme, WHITE, h, mix, with_alpha},
    wave::R,
};

fn base(
    id: &'static str,
    label: impl IntoElement,
    bg: Color,
    fg: Color,
    hover: Color,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .gap_1()
        .min_w(px(24.))
        .px_2()
        .h(px(24.))
        .rounded(px(4.))
        .bg(h(bg))
        .text_color(h(fg))
        .hover(move |s| s.bg(h(hover)))
        .cursor_pointer()
        .child(label)
}

/// Runs `on_press` on mouse down, and redraws.
pub fn on_press(
    el: Stateful<gpui::Div>,
    cx: &mut Context<Tonique>,
    f: impl Fn(&mut Tonique, &mut Context<Tonique>) + 'static,
) -> Stateful<gpui::Div> {
    el.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            f(this, cx);
            cx.notify();
        }),
    )
}

/// A button lit in the accent while `on`.
pub fn toggle_button(
    id: &'static str,
    label: impl IntoElement,
    on: bool,
    t: &Theme,
    cx: &mut Context<Tonique>,
    f: impl Fn(&mut Tonique, &mut Context<Tonique>) + 'static,
) -> Stateful<gpui::Div> {
    let el = if on {
        base(
            id,
            label,
            t.accent,
            t.text_on_accent,
            mix(t.accent, WHITE, 0.2),
        )
    } else {
        base(id, label, t.bg_control, t.text, t.bg_control_hover)
    };
    on_press(el, cx, f)
}

/// Tonique's `FlatButton`: a solid control fill (the accent while
/// `selected`), or for a `ghost` no fill until hovered and muted text (the
/// accent's while `selected`). `color` overrides the text's. Disabled, it
/// shows at half strength and does nothing.
pub struct Flat {
    pub id: &'static str,
    pub ghost: bool,
    pub selected: bool,
    pub enabled: bool,
    pub color: Option<Color>,
    pub w: f32,
    pub h: f32,
}

impl Flat {
    pub fn new(id: &'static str, size: f32) -> Self {
        Self {
            id,
            ghost: false,
            selected: false,
            enabled: true,
            color: None,
            w: size,
            h: size,
        }
    }
    pub fn ghost(id: &'static str, size: f32) -> Self {
        Self {
            ghost: true,
            ..Self::new(id, size)
        }
    }
    pub fn selected(self, selected: bool) -> Self {
        Self { selected, ..self }
    }
    pub fn enabled(self, enabled: bool) -> Self {
        Self { enabled, ..self }
    }
    pub fn color(self, color: Color) -> Self {
        Self {
            color: Some(color),
            ..self
        }
    }

    pub fn show(
        self,
        label: impl IntoElement,
        t: &Theme,
        cx: &mut Context<Tonique>,
        f: impl Fn(&mut Tonique, &mut Context<Tonique>) + 'static,
    ) -> Stateful<gpui::Div> {
        let transparent = Color::rgba(0, 0, 0, 0);
        let (fill, fg, hover) = match (self.ghost, self.selected) {
            (false, false) => (t.bg_control, t.text, t.bg_control_hover),
            (false, true) => (t.accent, t.text_on_accent, t.accent_hover),
            (true, false) => (transparent, t.text_muted, t.hover_overlay),
            (true, true) => (transparent, t.accent, t.hover_overlay),
        };
        let fg = self.color.unwrap_or(fg);
        let half = |c: Color| with_alpha(c, c.a / 2);
        let el = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(px(self.w))
            .h(px(self.h))
            .rounded(px(1.))
            .child(label);
        if !self.enabled {
            return el.bg(h(half(fill))).text_color(h(half(fg)));
        }
        on_press(
            el.bg(h(fill))
                .text_color(h(fg))
                .hover(move |s| s.bg(h(hover)))
                .cursor_pointer(),
            cx,
            f,
        )
    }
}

/// A horizontal slider at `value` (0 to 1) over `r`.
pub fn paint_slider(window: &mut Window, r: R, value: f32, t: &Theme) {
    let value = value.clamp(0., 1.);
    let mid = r.y + r.h / 2.;
    let track = R::new(r.x, mid - 2., r.w, 4.);
    window.paint_quad(gpui::fill(track.bounds(), h(t.bg_control)).corner_radii(px(2.)));
    window.paint_quad(
        gpui::fill(R::new(r.x, mid - 2., r.w * value, 4.).bounds(), h(t.accent))
            .corner_radii(px(2.)),
    );
    let knob = R::new(r.x + r.w * value - 5., mid - 5., 10., 10.);
    window.paint_quad(gpui::fill(knob.bounds(), h(t.text)).corner_radii(px(5.)));
}

/// One line of text, its top left at `(x, y)` in window coordinates.
/// Returns its width.
#[allow(clippy::too_many_arguments)]
pub fn paint_text(
    window: &mut Window,
    cx: &mut gpui::App,
    text: impl Into<SharedString>,
    x: f32,
    y: f32,
    size: f32,
    color: Hsla,
    font: Font,
) -> f32 {
    let text = text.into();
    if text.is_empty() {
        return 0.;
    }
    let run = TextRun {
        len: text.len(),
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(text, px(size), &[run], None);
    let width = f32::from(line.width);
    line.paint(
        point(px(x), px(y)),
        px(size * 1.25),
        gpui::TextAlign::Left,
        None,
        window,
        cx,
    )
    .ok();
    width
}
