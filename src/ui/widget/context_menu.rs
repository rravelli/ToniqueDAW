use crate::ui::{font::PHOSPHOR_REGULAR, theme::ThemeExt};
use egui::{
    Align2, Color32, FontId, Label, PopupCloseBehavior, Response, RichText, Sense, Stroke, Ui,
    Vec2, Widget,
    containers::menu::{MenuConfig, MenuState, SubMenu},
};
use egui_phosphor::fill::CARET_RIGHT;

const FONT_SIZE: f32 = 9.0;

pub struct ContextMenuButton {
    icon: String,
    text: String,
    text_color: Option<Color32>,
    submenu: bool,
    /// Highlighted while its submenu is open.
    open: bool,
}

impl ContextMenuButton {
    pub fn new(icon: &str, text: &str) -> Self {
        Self {
            icon: icon.into(),
            text: text.into(),
            text_color: None,
            submenu: false,
            open: false,
        }
    }

    pub fn text_color(mut self, color: Color32) -> Self {
        self.text_color = Some(color);
        self
    }

    pub fn submenu<R>(mut self, ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> Response {
        self.submenu = true;
        // The id the button is about to get, which names its submenu.
        let id = ui.next_auto_id();
        self.open = MenuState::from_ui(ui, |state, _| {
            state.open_item == Some(SubMenu::id_from_widget_id(id))
        });
        let response = self.ui(ui);
        debug_assert_eq!(response.id, id, "submenu open state read for the wrong id");

        // Items decide when to close (`ui.close()`), so a submenu can hold
        // controls that take several clicks.
        SubMenu::default()
            .config(MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside))
            .show(ui, &response, content);
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

        let theme = ui.app_theme();
        // Paint hover/click background
        if response.hovered() || response.highlighted() || self.open {
            let fill = if response.clicked() {
                theme.bg_control_hover
            } else {
                theme.bg_control
            };
            ui.painter().rect_filled(rect, 2.0, fill);
        }
        let text_color = self.text_color.unwrap_or(theme.text);

        // Draw the icon + text manually inside that region
        let icon_font = FontId::new(FONT_SIZE, egui::FontFamily::Name(PHOSPHOR_REGULAR.into()));
        let text_font = FontId::new(FONT_SIZE, egui::FontFamily::Proportional);

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
            text_color,
        );

        // Text
        painter.text(
            egui::pos2(text_x, center_y),
            Align2::LEFT_CENTER,
            self.text,
            text_font.clone(),
            text_color,
        );

        if self.submenu {
            painter.text(
                egui::pos2(rect.right() - 4., center_y),
                Align2::RIGHT_CENTER,
                CARET_RIGHT,
                icon_font,
                text_color,
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

pub struct ContextMenuLabel {
    text: String,
}

impl ContextMenuLabel {
    pub fn new(text: impl ToString) -> Self {
        Self {
            text: text.to_string(),
        }
    }
}

impl Widget for ContextMenuLabel {
    fn ui(self, ui: &mut Ui) -> Response {
        let text_font = FontId::new(FONT_SIZE, egui::FontFamily::Proportional);
        let res = ui.add(Label::new(
            RichText::new(self.text)
                .font(text_font)
                .color(ui.app_theme().text_muted),
        ));
        ui.add_space(3.0);
        res
    }
}
