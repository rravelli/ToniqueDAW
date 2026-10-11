//! Dialogs and windows over the app: one look for every prompt, notice and
//! window.

use crate::ui::{
    theme::{ThemeExt, font_size},
    widget::flat_button::FlatButton,
};
use egui::{FontId, Frame, Id, Margin, Modal, RichText, Stroke, Ui, vec2};

const BUTTON_HEIGHT: f32 = 24.;

/// A modal dialog on a dimmed window. Escape or a click outside asks it to
/// close: check `should_close` on the response.
pub fn modal(ui: &Ui, id: &str) -> Modal {
    Modal::new(Id::new(id))
        .backdrop_color(ui.app_theme().shadow)
        .frame(frame(ui))
}

/// The frame of dialogs and windows.
pub fn frame(ui: &Ui) -> Frame {
    let theme = ui.app_theme();
    Frame::new()
        .fill(theme.bg_raised)
        .stroke(Stroke::new(1., theme.border))
        .corner_radius(6)
        .inner_margin(Margin::same(18))
}

/// The dialog's question or headline.
pub fn title(ui: &mut Ui, text: impl Into<String>) {
    ui.label(RichText::new(text).size(font_size::HEADING).strong());
}

/// Explanation under the title.
pub fn body(ui: &mut Ui, text: impl Into<String>) {
    let muted = ui.app_theme().text_muted;
    ui.label(RichText::new(text).color(muted));
}

/// A dialog button; `.selected(true)` for the main action.
pub fn button(text: &str) -> FlatButton {
    FlatButton::new(text)
        .size(vec2(0., BUTTON_HEIGHT))
        .padding(12.)
        .font(FontId::proportional(font_size::BODY))
        .border_radius(3.)
}
