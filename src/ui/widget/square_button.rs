use egui::{Align2, Color32, CursorIcon, FontId, Sense, Stroke, Vec2, Widget};

pub struct SquareButton {
    size: f32,
    bg_color: Color32,
    // Text
    text: String,
    text_color: Color32,
    font: FontId,
}

impl SquareButton {
    pub fn new(text: &str) -> Self {
        Self {
            size: 15.,
            bg_color: Color32::from_gray(100),
            text: text.into(),
            font: FontId::proportional(8.),
            text_color: Color32::WHITE,
        }
    }
    pub fn fill(mut self, bg: Color32) -> Self {
        self.bg_color = bg;
        self
    }
    pub fn font(mut self, font_id: FontId) -> Self {
        self.font = font_id;
        self
    }
    pub fn color(mut self, color: Color32) -> Self {
        self.text_color = color;
        self
    }
    pub fn sized(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

impl Widget for SquareButton {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (mut res, painter) =
            ui.allocate_painter(Vec2::new(self.size, self.size), Sense::click());
        let rect = res.rect;
        let mut curr_color = self.bg_color;
        let mut stroke = Stroke::NONE;
        if res.hovered() {
            curr_color = self.bg_color.gamma_multiply(0.8);
        }
        if res.has_focus() {
            stroke = Stroke::new(1.0, Color32::from_white_alpha(200));
        }
        // Paint widget
        painter.rect(rect, 1.0, curr_color, stroke, egui::StrokeKind::Inside);

        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            self.text,
            self.font,
            self.text_color,
        );
        // Update response
        res = res.on_hover_cursor(CursorIcon::PointingHand);

        res
    }
}
