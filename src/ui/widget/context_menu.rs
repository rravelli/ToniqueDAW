use egui::{
    Align2, Color32, FontId, Response, Sense, Stroke, Ui, Vec2, Widget,
    containers::menu::{MenuState, SubMenu},
};
use egui_phosphor::fill::CARET_RIGHT;

use crate::ui::font::PHOSPHOR_REGULAR;

pub struct ContextMenuButton {
    icon: String,
    text: String,
    text_color: Color32,
    submenu: bool,
}

impl ContextMenuButton {
    pub fn new(icon: &str, text: &str) -> Self {
        Self {
            icon: icon.into(),
            text: text.into(),
            text_color: Color32::WHITE,
            submenu: false,
        }
    }

    pub fn text_color(mut self, color: Color32) -> Self {
        self.text_color = color;
        self
    }

    pub fn submenu<R>(mut self, ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> Response {
        self.submenu = true;
        let response = self.ui(ui);

        let my_id = ui.next_auto_id();
        let open = MenuState::from_ui(ui, |state, _| {
            state.open_item == Some(SubMenu::id_from_widget_id(my_id))
        });
        let inactive = ui.style().visuals.widgets.inactive;
        // TODO(lucasmerlin) add `open` function to `Button`
        if open {
            ui.style_mut().visuals.widgets.inactive = ui.style().visuals.widgets.open;
        }
        ui.style_mut().visuals.widgets.inactive = inactive;

        SubMenu::default().show(ui, &response, content);
        response
    }
}

impl Widget for ContextMenuButton {
    fn ui(self, ui: &mut Ui) -> Response {
        // Define the desired height and width for the entire button
        let height = 16.0;
        let width = ui.available_width().min(120.); // Fill full width of the menu
        let desired_size = Vec2::new(width, height);

        // Allocate a rectangular region for interaction
        let (rect, response) = ui.allocate_exact_size(desired_size, Sense::all());

        // Paint hover/click background
        if response.hovered() || response.highlighted() {
            let fill = if response.clicked() {
                Color32::from_rgb(80, 80, 80)
            } else {
                Color32::from_rgb(60, 60, 60)
            };
            ui.painter().rect_filled(rect, 2.0, fill);
        }

        // Draw the icon + text manually inside that region
        let icon_font = FontId::new(10.0, egui::FontFamily::Name(PHOSPHOR_REGULAR.into()));
        let text_font = FontId::new(10.0, egui::FontFamily::Proportional);

        let icon_x = rect.left() + 3.0;
        let text_x = rect.left() + 18.0;
        let center_y = rect.center().y;

        let painter = ui.painter();

        // Icon
        painter.text(
            egui::pos2(icon_x, center_y),
            Align2::LEFT_CENTER,
            self.icon,
            icon_font.clone(),
            self.text_color,
        );

        // Text
        painter.text(
            egui::pos2(text_x, center_y),
            Align2::LEFT_CENTER,
            self.text,
            text_font.clone(),
            self.text_color,
        );

        if self.submenu {
            painter.text(
                egui::pos2(rect.right() - 4., center_y),
                Align2::RIGHT_CENTER,
                CARET_RIGHT,
                icon_font,
                self.text_color,
            );
        };
        response
        // Change cursor on hover
        // response.on_hover_cursor(CursorIcon::PointingHand)
    }
}

pub struct ContextMenuSeparator;

impl ContextMenuSeparator {
    pub fn new() -> Self {
        Self
    }
}

impl Widget for ContextMenuSeparator {
    fn ui(self, ui: &mut Ui) -> Response {
        // Add some vertical spacing before and after
        ui.add_space(2.0);

        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width().min(120.), 1.0),
            egui::Sense::hover(),
        );
        let stroke_color = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let stroke = Stroke::new(1.0, stroke_color);

        ui.painter()
            .line_segment([rect.left_center(), rect.right_center()], stroke);

        ui.add_space(2.0);

        response
    }
}
