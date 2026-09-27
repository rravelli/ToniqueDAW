use egui::{Align2, Color32, FontId, Response, Sense, Stroke, Ui, Widget, pos2, vec2};

use crate::ui::theme::PRIMARY_COLOR;

const HEIGHT: f32 = 20.;
const GAP: f32 = 8.;

/// Section title followed by a rule spanning the available width.
pub struct SectionHeader {
    title: String,
}

impl SectionHeader {
    pub fn new(title: impl ToString) -> Self {
        Self {
            title: title.to_string(),
        }
    }
}

impl Widget for SectionHeader {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT), Sense::hover());
        let painter = ui.painter();
        let text = painter.text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            self.title,
            FontId::proportional(12.),
            PRIMARY_COLOR,
        );
        if text.right() + GAP < rect.right() {
            painter.line_segment(
                [
                    pos2(text.right() + GAP, rect.center().y),
                    pos2(rect.right(), rect.center().y),
                ],
                Stroke::new(1., Color32::from_gray(70)),
            );
        }
        response
    }
}
