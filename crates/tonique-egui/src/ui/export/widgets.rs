//! The export dialogs' building blocks.

use egui::{
    Color32, FontFamily, FontId, Painter, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui,
    vec2,
};

use crate::ui::{
    font::PHOSPHOR_REGULAR,
    theme::{Theme, ThemeExt, font_size},
    widget::{
        dialog::{self},
        flat_button::FlatButton,
    },
};

use super::CHOICE_HEIGHT;

/// The title and, muted under it, a subtitle.
pub(super) fn header(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.;
        dialog::title(ui, title);
        if !subtitle.is_empty() {
            ui.label(
                RichText::new(subtitle)
                    .color(ui.app_theme().text_muted)
                    .size(font_size::LABEL),
            );
        }
    });
}

/// One line with an icon, in `color`.
pub(super) fn note(ui: &mut Ui, icon: &str, color: Color32, text: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.;
        ui.label(
            RichText::new(icon)
                .family(FontFamily::Name(PHOSPHOR_REGULAR.into()))
                .color(color),
        );
        ui.label(RichText::new(text).color(color).size(font_size::LABEL));
    });
}

/// Buttons side by side sharing the width; the one matching `value` is
/// highlighted.
pub(super) fn choices<T: PartialEq + Copy, const N: usize>(
    ui: &mut Ui,
    value: &mut T,
    options: [(T, String); N],
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.;
        let width = (ui.available_width() - 4. * (N - 1) as f32) / N as f32;
        for (option, label) in options {
            let clicked = ui
                .add(
                    FlatButton::new(label)
                        .size(vec2(width, CHOICE_HEIGHT))
                        .font(FontId::proportional(12.))
                        .border_radius(3.)
                        .selected(*value == option),
                )
                .clicked();
            if clicked {
                *value = option;
            }
        }
    });
}

/// A card to draw into, `height` tall and as wide as the dialog.
pub(super) fn card(
    ui: &mut Ui,
    height: f32,
    theme: &Theme,
    sense: Sense,
) -> (Rect, Painter, Response) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), sense);
    let painter = ui.painter_at(rect);
    painter.rect(
        rect,
        4.,
        theme.bg_deep,
        Stroke::new(1., theme.border),
        StrokeKind::Inside,
    );
    (rect, painter, response)
}
