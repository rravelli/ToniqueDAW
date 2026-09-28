use egui::{Color32, Sense, Stroke, Vec2, Widget};

/// A filled bar, e.g. a track's colour beside its header.
pub struct ColorBar {
    size: Vec2,
    color: Color32,
}

impl ColorBar {
    pub fn new(size: Vec2) -> Self {
        Self {
            size,
            color: Color32::TRANSPARENT,
        }
    }
    pub fn color(mut self, color: Color32) -> Self {
        self.color = color;
        self
    }
}

impl Widget for ColorBar {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (res, painter) = ui.allocate_painter(self.size, Sense::click());
        let rect = res.rect;
        // Paint widget
        painter.rect(
            rect,
            0.,
            self.color,
            Stroke::NONE,
            egui::StrokeKind::Outside,
        );

        res
    }
}
