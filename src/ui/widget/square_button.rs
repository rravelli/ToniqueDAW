use egui::{
    Align2, Color32, CursorIcon, FontFamily, FontId, RichText, Sense, Stroke, Vec2, Widget, vec2,
};

pub struct SquareButton {
    size: Vec2,
    bg_color: Color32,
    border_radius: f32,
    hover_color: Option<Color32>,
    // Text
    text: String,
    text_color: Color32,
    font: FontId,
    // Tooltip
    tooltip_text: String,
}

impl SquareButton {
    pub fn new(text: impl ToString) -> Self {
        Self {
            size: vec2(15., 15.),
            bg_color: Color32::from_gray(100),
            text: text.to_string(),
            font: FontId::proportional(8.),
            text_color: Color32::WHITE,
            tooltip_text: "".to_string(),
            border_radius: 1.0,
            hover_color: None,
        }
    }
    pub fn ghost(text: impl ToString) -> Self {
        Self {
            bg_color: Color32::TRANSPARENT,
            size: vec2(15., 15.),
            border_radius: 1.0,
            font: FontId::proportional(8.),
            hover_color: Some(Color32::from_white_alpha(30)),
            text: text.to_string(),
            text_color: Color32::from_gray(180),
            tooltip_text: "".to_string(),
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
    pub fn square(mut self, size: f32) -> Self {
        self.size = vec2(size, size);
        self
    }
    pub fn size(mut self, size: Vec2) -> Self {
        self.size = size;
        self
    }
    pub fn family(mut self, family: FontFamily) -> Self {
        self.font.family = family;
        self
    }
    pub fn tooltip(mut self, text: impl ToString) -> Self {
        self.tooltip_text = text.to_string();
        self
    }
    pub fn border_radius(mut self, border_radius: f32) -> Self {
        self.border_radius = border_radius;
        self
    }
    pub fn hover_color(mut self, color: Color32) -> Self {
        self.hover_color = Some(color);
        self
    }
}

impl Widget for SquareButton {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (mut res, painter) = ui.allocate_painter(self.size, Sense::click());
        let rect = res.rect;
        let mut curr_color = self.bg_color;
        let mut stroke = Stroke::NONE;
        if res.hovered() {
            curr_color = self
                .hover_color
                .unwrap_or(self.bg_color.blend(Color32::from_white_alpha(40)));
        }
        if res.has_focus() {
            stroke = Stroke::new(1.0, Color32::from_white_alpha(200));
        }
        // Paint widget
        painter.rect(
            rect,
            self.border_radius,
            curr_color,
            stroke,
            egui::StrokeKind::Inside,
        );

        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            self.text,
            self.font,
            self.text_color,
        );
        // Update response
        res = res.on_hover_cursor(CursorIcon::PointingHand);

        if !self.tooltip_text.is_empty() {
            res = res.on_hover_text(
                RichText::new(self.tooltip_text)
                    .color(Color32::WHITE)
                    .font(FontId::new(8., egui::FontFamily::Proportional)),
            )
        }

        res
    }
}
