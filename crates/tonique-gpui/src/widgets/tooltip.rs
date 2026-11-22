//! A short name shown under the pointer, for icon-only controls.

use gpui::{AnyView, App, Context, Render, SharedString, Window, div, prelude::*, px};

use crate::theme::{FONT_UI, Theme, hsla, size};

struct Tooltip {
    text: SharedString,
    theme: Theme,
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        div()
            .px(px(6.))
            .py(px(2.))
            .rounded(px(3.))
            .bg(hsla(theme.bg_raised))
            .border_1()
            .border_color(hsla(theme.border))
            .font_family(FONT_UI)
            .text_size(px(size::SMALL))
            .text_color(hsla(theme.text))
            .child(self.text.clone())
    }
}

/// Builds the tooltip showing `text`, for an element's `.tooltip(..)`.
pub fn tooltip(
    text: &'static str,
    theme: &Theme,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let theme = theme.clone();
    move |_, app| {
        app.new(|_| Tooltip {
            text: text.into(),
            theme: theme.clone(),
        })
        .into()
    }
}
