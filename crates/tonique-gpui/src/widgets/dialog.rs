//! The parts of dialogs, as the egui app's `widget/dialog.rs`: the frame,
//! a title, explanation under it, and buttons.

use gpui::{SharedString, Stateful, div, prelude::*, px};

use crate::theme::{Theme, hsla, size};

const PADDING: f32 = 18.;
const RADIUS: f32 = 6.;
const BUTTON_HEIGHT: f32 = 24.;
const BUTTON_PADDING: f32 = 12.;
const BUTTON_RADIUS: f32 = 3.;

/// The frame of a dialog, `width` wide.
pub fn dialog_frame(width: f32, theme: &Theme) -> gpui::Div {
    div()
        .w(px(width))
        .p(px(PADDING))
        .rounded(px(RADIUS))
        .bg(hsla(theme.bg_raised))
        .border_1()
        .border_color(hsla(theme.border))
        .shadow_lg()
        .text_size(px(size::BODY))
        .text_color(hsla(theme.text))
}

pub fn dialog_title(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(size::HEADING))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(text.into())
}

/// Explanation under the title.
pub fn dialog_body(text: impl Into<SharedString>, theme: &Theme) -> gpui::Div {
    div().text_color(hsla(theme.text_muted)).child(text.into())
}

/// A dialog's button, in the accent when it's the main action
/// (`selected`). The caller handles its press.
pub fn dialog_button(
    id: &'static str,
    text: impl Into<SharedString>,
    selected: bool,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let (fill, ink, hover) = if selected {
        (theme.accent, theme.text_on_accent, theme.accent_hover)
    } else {
        (theme.bg_control, theme.text, theme.bg_control_hover)
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(BUTTON_HEIGHT))
        .px(px(BUTTON_PADDING))
        .rounded(px(BUTTON_RADIUS))
        .bg(hsla(fill))
        .text_color(hsla(ink))
        .hover(move |style| style.bg(hsla(hover)))
        .cursor_pointer()
        .child(text.into())
}
