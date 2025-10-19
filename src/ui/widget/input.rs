use egui::{
    Align2, Color32, FontId, Margin, Painter, Pos2, Rangef, Rect, Response, Sense, Stroke,
    StrokeKind, TextEdit, UiBuilder, Vec2,
};
use egui_phosphor::fill::{CARET_DOWN, CARET_UP};

const BUTTON_SIZE: Vec2 = Vec2::new(14., 8.0);

const BUTTONS_GAP: f32 = 1.0;
const STROKE_SIZE: f32 = 1.0;
pub struct NumberInput {
    size: Vec2,
    bg_color: Color32,
    text_color: Color32,
    font: FontId,

    edit_mode: bool,
    buffer: String,
    pub value: f32,
    range: Rangef,
}

impl NumberInput {
    pub fn new(size: Vec2) -> Self {
        Self {
            size,
            bg_color: Color32::from_gray(80),
            text_color: Color32::WHITE,
            font: FontId::new(12., egui::FontFamily::Proportional),
            edit_mode: false,
            buffer: "".into(),
            value: 0.,
            range: Rangef::new(0., 1000.),
        }
    }

    pub fn with_range(mut self, range: Rangef) -> Self {
        self.range = range;
        self
    }

    pub fn fill(mut self, fill: Color32) -> Self {
        self.bg_color = fill;
        self
    }

    pub fn text_color(mut self, color: Color32) -> Self {
        self.text_color = color;
        self
    }

    fn parse_to_int(&mut self) {
        let result = self.buffer.parse::<f32>();
        match result {
            Ok(number) => self.value = number.clamp(self.range.min, self.range.max),
            Err(_) => {}
        }
    }

    fn parse_to_string(&mut self, value: f32) -> String {
        format!("{:.1}", value)
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Response {
        let (res, painter) = ui.allocate_painter(self.size, Sense::click());
        let rect = res.rect;
        let curr_color = self.bg_color;
        // Paint widget
        painter.rect_filled(rect, 1.0, curr_color);

        let mut text = self.parse_to_string(self.value);
        // Text
        let text_edit_rect = Rect::from_min_max(
            rect.min,
            Pos2::new(rect.max.x - BUTTON_SIZE.x - STROKE_SIZE, rect.max.y),
        );
        let text_edit = ui
            .scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                TextEdit::singleline(if self.edit_mode {
                    &mut self.buffer
                } else {
                    &mut text
                })
                .desired_width(text_edit_rect.width())
                .frame(false)
                .font(self.font.clone())
                .interactive(self.edit_mode)
                .horizontal_align(egui::Align::Center)
                .margin(Margin::ZERO)
                .text_color(self.text_color)
                .show(ui)
            })
            .inner
            .response;

        if text_edit.lost_focus() {
            self.edit_mode = false;
            self.parse_to_int();
        }

        let offset = BUTTONS_GAP / 2.;
        // First button
        let rect1 = Rect::from_min_max(
            Pos2::new(
                rect.max.x - BUTTON_SIZE.x - STROKE_SIZE,
                rect.center().y - BUTTON_SIZE.y - offset,
            ),
            Pos2::new(rect.max.x - STROKE_SIZE, rect.center().y - offset),
        );
        let res1 = self.make_button(ui, &painter, rect1, CARET_UP);
        // Second button
        let rect2 = Rect::from_min_max(
            Pos2::new(
                rect.max.x - BUTTON_SIZE.x - STROKE_SIZE,
                rect.center().y + offset,
            ),
            Pos2::new(
                rect.max.x - STROKE_SIZE,
                rect.center().y + BUTTON_SIZE.y + offset,
            ),
        );
        let res2 = self.make_button(ui, &painter, rect2, CARET_DOWN);
        let focused =
            res.has_focus() || res1.has_focus() || res2.has_focus() || text_edit.has_focus();
        if focused {
            painter.rect_stroke(
                rect,
                1.0,
                Stroke::new(STROKE_SIZE, Color32::from_white_alpha(200)),
                StrokeKind::Inside,
            );
        }

        if res.double_clicked() {
            self.edit_mode = true;
            self.buffer = self.parse_to_string(self.value);
            text_edit.request_focus();
        }
        if res.clicked() {
            res.request_focus();
        }
        if res1.clicked() || (focused && ui.input(|i| i.key_pressed(egui::Key::ArrowUp))) {
            res1.request_focus();
            self.value += 1.0;
        }
        if res2.clicked() || (focused && ui.input(|i| i.key_pressed(egui::Key::ArrowDown))) {
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
            res1.request_focus();
            self.value -= 1.0;
        }
        // Make sure the input has the correct size
        ui.allocate_rect(rect, Sense::empty());

        return res;
    }

    fn make_button(
        &self,
        ui: &mut egui::Ui,
        painter: &Painter,
        rect: Rect,
        icon: &str,
    ) -> Response {
        let res = ui.allocate_rect(rect, Sense::click());
        let mut bg = Color32::from_gray(30);
        if res.hovered() {
            bg = bg.gamma_multiply(0.7);
        }

        painter.rect_filled(rect, 1.0, bg);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            icon,
            FontId::new(8., egui::FontFamily::Proportional),
            Color32::WHITE,
        );

        res
    }
}
