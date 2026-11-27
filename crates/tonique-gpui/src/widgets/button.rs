//! Buttons: V's `FlatButton`, a toggle lit in the accent, and the
//! press handler they share.

use gpui::{Context, MouseButton, Stateful, div, prelude::*, px};

use crate::theme::{Color, Theme, WHITE, hsla, icon, icon_fill, mix, with_alpha};

/// Runs `action` on mouse down, and redraws.
pub fn on_press<V: 'static>(
    element: Stateful<gpui::Div>,
    context: &mut Context<V>,
    action: impl Fn(&mut V, &mut Context<V>) + 'static,
) -> Stateful<gpui::Div> {
    element.on_mouse_down(
        MouseButton::Left,
        context.listener(move |this, _, _, context| {
            context.stop_propagation();
            action(this, context);
            context.notify();
        }),
    )
}

/// An icon at `size`, filled or not.
pub fn glyph(glyph_name: &'static str, size: f32, fill: bool) -> gpui::Div {
    let element = if fill {
        icon_fill(glyph_name)
    } else {
        icon(glyph_name)
    };
    element.text_size(px(size)).line_height(px(size))
}

/// A button lit in the accent while `on`.
pub fn toggle_button<V: 'static>(
    id: &'static str,
    label: impl IntoElement,
    on: bool,
    theme: &Theme,
    context: &mut Context<V>,
    action: impl Fn(&mut V, &mut Context<V>) + 'static,
) -> Stateful<gpui::Div> {
    let (background, foreground, hover) = if on {
        (
            theme.accent,
            theme.text_on_accent,
            mix(theme.accent, WHITE, 0.2),
        )
    } else {
        (theme.bg_control, theme.text, theme.bg_control_hover)
    };
    let element = div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .gap_1()
        .min_w(px(24.))
        .px_2()
        .h(px(24.))
        .rounded(px(4.))
        .bg(hsla(background))
        .text_color(hsla(foreground))
        .hover(move |style| style.bg(hsla(hover)))
        .cursor_pointer()
        .child(label);
    on_press(element, context, action)
}

/// V's `FlatButton`: a solid control fill (the accent while
/// `selected`), or for a `ghost` no fill until hovered and muted text (the
/// accent's while `selected`). `color` overrides the text's. Disabled, it
/// shows at half strength and does nothing.
pub struct Flat {
    id: gpui::ElementId,
    ghost: bool,
    selected: bool,
    enabled: bool,
    color: Option<Color>,
    width: Option<f32>,
    /// As wide as its label and this much each side of it, rather than
    /// `width`.
    padding: Option<f32>,
    height: f32,
}

impl Flat {
    /// A square button `size` wide.
    pub fn new(id: impl Into<gpui::ElementId>, size: f32) -> Self {
        Self {
            id: id.into(),
            ghost: false,
            selected: false,
            enabled: true,
            color: None,
            width: Some(size),
            padding: None,
            height: size,
        }
    }
    pub fn ghost(id: impl Into<gpui::ElementId>, size: f32) -> Self {
        Self {
            ghost: true,
            ..Self::new(id, size)
        }
    }
    /// As wide as its parent.
    pub fn full_width(self) -> Self {
        Self {
            width: None,
            ..self
        }
    }
    /// As wide as its label, `padding` each side of it.
    pub fn fit(self, padding: f32) -> Self {
        Self {
            padding: Some(padding),
            ..self
        }
    }
    pub fn width(self, width: f32) -> Self {
        Self {
            width: Some(width),
            ..self
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

    /// The button around `label`, without an action: for a caller that
    /// handles the press itself (it needs the event).
    pub fn element(self, label: impl IntoElement, theme: &Theme) -> Stateful<gpui::Div> {
        let transparent = Color::rgba(0, 0, 0, 0);
        let (fill, foreground, hover) = match (self.ghost, self.selected) {
            (false, false) => (theme.bg_control, theme.text, theme.bg_control_hover),
            (false, true) => (theme.accent, theme.text_on_accent, theme.accent_hover),
            (true, false) => (transparent, theme.text_muted, theme.hover_overlay),
            (true, true) => (transparent, theme.accent, theme.hover_overlay),
        };
        let foreground = self.color.unwrap_or(foreground);
        let element = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap_1()
            .h(px(self.height))
            .rounded(px(1.))
            .child(label);
        let element = match (self.padding, self.width) {
            (Some(padding), _) => element.px(px(padding)),
            (None, Some(width)) => element.w(px(width)),
            (None, None) => element.w_full(),
        };
        if !self.enabled {
            let half = |color: Color| with_alpha(color, color.a / 2);
            return element
                .bg(hsla(half(fill)))
                .text_color(hsla(half(foreground)));
        }
        element
            .bg(hsla(fill))
            .text_color(hsla(foreground))
            .hover(move |style| style.bg(hsla(hover)))
            .cursor_pointer()
    }

    /// The button around `label`, running `action` when pressed.
    pub fn show<V: 'static>(
        self,
        label: impl IntoElement,
        theme: &Theme,
        context: &mut Context<V>,
        action: impl Fn(&mut V, &mut Context<V>) + 'static,
    ) -> Stateful<gpui::Div> {
        let enabled = self.enabled;
        let element = self.element(label, theme);
        if enabled {
            on_press(element, context, action)
        } else {
            element
        }
    }
}
