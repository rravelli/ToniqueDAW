use egui::{Color32, Sense, Stroke, Vec2, Widget};

pub struct Rectangle {
    size: Vec2,
    bg_color: Color32,
}

impl Rectangle {
    pub fn new(size: Vec2) -> Self {
        Self {
            size,
            bg_color: Color32::GRAY,
        }
    }
    pub fn fill(mut self, bg: Color32) -> Self {
        self.bg_color = bg;
        self
    }
}

impl Widget for Rectangle {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (res, painter) = ui.allocate_painter(self.size, Sense::click());
        let rect = res.rect;
        // Paint widget
        painter.rect(
            rect,
            0.,
            self.bg_color,
            Stroke::NONE,
            egui::StrokeKind::Outside,
        );

        res
    }
}
