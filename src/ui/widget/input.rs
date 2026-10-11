use crate::ui::theme::font_size;
use egui::{
    Align2, FontId, Margin, Painter, Pos2, Rangef, Rect, Response, Sense, Stroke, StrokeKind,
    TextEdit, UiBuilder, Vec2,
};
use egui_phosphor::fill::{CARET_DOWN, CARET_UP};

use crate::ui::theme::ThemeExt;

const BUTTON_SIZE: Vec2 = Vec2::new(14., 8.0);

const BUTTONS_GAP: f32 = 1.0;
const STROKE_SIZE: f32 = 1.0;
/// Points of vertical drag per step, when draggable.
const DRAG_PIXELS_PER_STEP: f32 = 3.0;
/// Step while dragging with Shift held; a whole unit otherwise.
const FINE_STEP: f32 = 0.1;
pub struct NumberInput {
    size: Vec2,
    font: FontId,

    edit_mode: bool,
    buffer: String,
    pub value: f32,
    range: Rangef,
    decimals: usize,
    suffix: String,
    /// Dragged up and down to change, without arrow buttons.
    draggable: bool,
    /// Drag not yet turned into a step.
    drag_rest: f32,
}

impl NumberInput {
    pub fn new(size: Vec2) -> Self {
        Self {
            size,
            font: FontId::new(font_size::BODY, egui::FontFamily::Proportional),
            edit_mode: false,
            buffer: "".into(),
            value: 0.,
            range: Rangef::new(0., 1000.),
            decimals: 1,
            suffix: String::new(),
            draggable: false,
            drag_rest: 0.,
        }
    }

    pub fn range(mut self, range: Rangef) -> Self {
        self.range = range;
        self
    }

    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals;
        self
    }

    /// Unit shown after the value (not while editing).
    pub fn suffix(mut self, suffix: impl ToString) -> Self {
        self.suffix = suffix.to_string();
        self
    }

    /// Drag up or down to change the value (Shift for finer steps), in
    /// place of the arrow buttons.
    pub fn draggable(mut self) -> Self {
        self.draggable = true;
        self
    }

    fn commit_buffer(&mut self) {
        let result = self.buffer.parse::<f32>();
        if let Ok(number) = result {
            self.value = number.clamp(self.range.min, self.range.max)
        }
    }

    fn format_value(&self, value: f32) -> String {
        format!("{:.*}", self.decimals, value)
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Response {
        let sense = if self.draggable {
            Sense::click_and_drag()
        } else {
            Sense::click()
        };
        let (res, painter) = ui.allocate_painter(self.size, sense);
        let rect = res.rect;
        let theme = ui.app_theme();
        // Paint widget
        painter.rect_filled(rect, 1.0, theme.bg_control);

        let mut text = self.format_value(self.value) + &self.suffix;
        // Text
        let text_edit_rect = if self.draggable {
            rect
        } else {
            Rect::from_min_max(
                rect.min,
                Pos2::new(rect.max.x - BUTTON_SIZE.x - STROKE_SIZE, rect.max.y),
            )
        };
        let text_edit = ui
            .scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                TextEdit::singleline(if self.edit_mode {
                    &mut self.buffer
                } else {
                    &mut text
                })
                .desired_width(text_edit_rect.width())
                .frame(egui::Frame::NONE)
                .font(self.font.clone())
                .interactive(self.edit_mode)
                .horizontal_align(egui::Align::Center)
                .margin(Margin::ZERO)
                .text_color(theme.text)
                .show(ui)
            })
            .inner
            .response;

        if text_edit.lost_focus() {
            self.edit_mode = false;
            self.commit_buffer();
        }
        if self.draggable {
            return self.drag_ui(ui, res, &painter, &text_edit);
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
                Stroke::new(STROKE_SIZE, theme.accent),
                StrokeKind::Inside,
            );
        }

        if res.double_clicked() {
            self.edit_mode = true;
            self.buffer = self.format_value(self.value);
            text_edit.request_focus();
        }
        if res.clicked() {
            res.request_focus();
        }
        if res1.clicked() || (focused && ui.input(|i| i.key_pressed(egui::Key::ArrowUp))) {
            res1.request_focus();
            self.value = (self.value + 1.0).clamp(self.range.min, self.range.max);
        }
        if res2.clicked() || (focused && ui.input(|i| i.key_pressed(egui::Key::ArrowDown))) {
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
            res1.request_focus();
            self.value = (self.value - 1.0).clamp(self.range.min, self.range.max);
        }
        // Make sure the input has the correct size
        ui.allocate_rect(rect, Sense::empty());

        res
    }

    /// Focus, editing and dragging for a draggable input.
    fn drag_ui(
        &mut self,
        ui: &mut egui::Ui,
        res: Response,
        painter: &Painter,
        text_edit: &Response,
    ) -> Response {
        let theme = ui.app_theme();
        let focused = res.has_focus() || text_edit.has_focus();
        if focused || res.dragged() {
            painter.rect_stroke(
                res.rect,
                1.0,
                Stroke::new(STROKE_SIZE, theme.accent),
                StrokeKind::Inside,
            );
        }
        if res.double_clicked() {
            self.edit_mode = true;
            self.buffer = self.format_value(self.value);
            text_edit.request_focus();
        }
        if res.clicked() {
            res.request_focus();
        }
        if res.drag_started() {
            self.drag_rest = 0.;
        }
        if res.dragged() {
            let step = if ui.input(|i| i.modifiers.shift) {
                FINE_STEP
            } else {
                1.0
            };
            self.drag_rest -= res.drag_delta().y / DRAG_PIXELS_PER_STEP;
            let steps = self.drag_rest.trunc();
            self.drag_rest -= steps;
            if steps != 0. {
                // Back on the step grid, so Shift's tenths don't linger.
                let value = ((self.value / step).round() + steps) * step;
                self.value = value.clamp(self.range.min, self.range.max);
            }
        }
        if focused && !self.edit_mode {
            let delta = ui.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    1.0
                } else if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    -1.0
                } else {
                    0.0
                }
            });
            self.value = (self.value + delta).clamp(self.range.min, self.range.max);
        }
        ui.allocate_rect(res.rect, Sense::empty());
        res.on_hover_cursor(egui::CursorIcon::ResizeVertical)
    }

    fn make_button(
        &self,
        ui: &mut egui::Ui,
        painter: &Painter,
        rect: Rect,
        icon: &str,
    ) -> Response {
        let res = ui.allocate_rect(rect, Sense::click());
        let theme = ui.app_theme();
        let bg = if res.hovered() {
            theme.bg_control_hover
        } else {
            theme.bg_raised
        };

        painter.rect_filled(rect, 1.0, bg);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            icon,
            FontId::new(8., egui::FontFamily::Proportional),
            theme.text_muted,
        );

        res
    }
}
