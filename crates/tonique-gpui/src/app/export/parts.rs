//! The export dialogs' building blocks.

use gpui::{Context, SharedString, div, prelude::*, px};

use crate::{
    app::Tonique,
    theme::{Color, Theme, hsla, size},
    widgets::{Flat, dialog_title, glyph},
};

use super::CHOICE_HEIGHT;

/// Room for a field's name before it.
const FIELD_LABEL_WIDTH: f32 = 48.;
const CHOICE_GAP: f32 = 4.;

/// The title and, muted under it, a subtitle.
pub(super) fn header(title: impl Into<SharedString>, subtitle: String, theme: &Theme) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(dialog_title(title))
        .when(!subtitle.is_empty(), |element| {
            element.child(small(subtitle, theme.text_muted))
        })
}

/// Text at the label size, in `color`.
pub(super) fn small(text: impl Into<SharedString>, color: Color) -> gpui::Div {
    div()
        .text_size(px(size::LABEL))
        .text_color(hsla(color))
        .child(text.into())
}

/// One line with an icon, in `color`.
pub(super) fn note(icon: &'static str, color: Color, text: impl Into<SharedString>) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .text_color(hsla(color))
        .child(glyph(icon, size::BODY, false))
        .child(small(text, color))
}

/// A field's name before it, muted.
pub(super) fn field_label(text: &'static str, theme: &Theme) -> gpui::Div {
    div()
        .flex_none()
        .w(px(FIELD_LABEL_WIDTH))
        .text_color(hsla(theme.text_muted))
        .child(text)
}

/// Buttons side by side sharing the width; the one matching `value` is
/// highlighted, and pressing one `set`s its option.
pub(super) fn choices<T: Copy + PartialEq + 'static>(
    id: &'static str,
    value: T,
    options: Vec<(T, String)>,
    theme: &Theme,
    context: &mut Context<Tonique>,
    set: fn(&mut Tonique, T),
) -> gpui::Div {
    let buttons = options
        .into_iter()
        .enumerate()
        .map(|(index, (option, label))| {
            let button = Flat::new((id, index), CHOICE_HEIGHT)
                .full_width()
                .selected(option == value)
                .show(
                    div().whitespace_nowrap().child(label),
                    theme,
                    context,
                    move |this, _| set(this, option),
                )
                .rounded(px(3.));
            div().flex_1().min_w_0().child(button)
        });
    div().flex().gap(px(CHOICE_GAP)).children(buttons)
}

/// A card to draw into, `height` tall and as wide as the dialog.
pub(super) fn card(height: f32, theme: &Theme) -> gpui::Div {
    div()
        .relative()
        .w_full()
        .h(px(height))
        .rounded(px(4.))
        .bg(hsla(theme.bg_deep))
        .border_1()
        .border_color(hsla(theme.border))
        .overflow_hidden()
}
