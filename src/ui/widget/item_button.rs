use egui::{Color32, Sense, Vec2, Widget};

use crate::ui::theme::PRIMARY_COLOR;

pub struct ItemButton {
    text: String,
    selected: bool,
}

impl ItemButton {
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

impl Widget for ItemButton {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 16.0),
            Sense::click_and_drag(),
        );

        let bg_color = if self.selected || response.has_focus() {
            PRIMARY_COLOR.gamma_multiply_u8(60)
        } else if response.hovered() {
            Color32::from_gray(40)
        } else {
            Color32::from_gray(30)
        };
        let painter = ui.painter_at(rect);

        painter.rect_filled(response.rect, 0., bg_color);

        let mut font_id = egui::TextStyle::Button.resolve(ui.style());
        font_id.size = 12.;

        let galley = ui
            .painter()
            .layout_no_wrap(self.text.to_string(), font_id, Color32::WHITE);

        painter.galley(
            response.rect.left_top() + Vec2::new(6.0, 1.0),
            galley,
            Color32::WHITE,
        );

        response
    }
}
