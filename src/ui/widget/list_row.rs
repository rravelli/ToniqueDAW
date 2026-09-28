use egui::{Sense, Vec2, Widget};

use crate::ui::theme::{ThemeExt, with_alpha};

pub struct ListRow {
    text: String,
    selected: bool,
}

impl ListRow {
    pub fn new(text: impl ToString) -> Self {
        Self {
            text: text.to_string(),
            selected: false,
        }
    }

    pub fn selected(mut self, val: bool) -> Self {
        self.selected = val;
        self
    }
}

impl Widget for ListRow {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 16.0),
            Sense::click_and_drag(),
        );

        let theme = ui.app_theme();
        let bg_color = if self.selected || response.has_focus() {
            with_alpha(theme.accent, 60)
        } else if response.hovered() {
            theme.hover_overlay
        } else {
            egui::Color32::TRANSPARENT
        };
        let painter = ui.painter_at(rect);

        painter.rect_filled(response.rect, 0., bg_color);

        let mut font_id = egui::TextStyle::Button.resolve(ui.style());
        font_id.size = 12.;

        let galley = ui
            .painter()
            .layout_no_wrap(self.text.to_string(), font_id, theme.text);

        painter.galley(
            response.rect.left_top() + Vec2::new(6.0, 1.0),
            galley,
            theme.text,
        );

        response
    }
}
