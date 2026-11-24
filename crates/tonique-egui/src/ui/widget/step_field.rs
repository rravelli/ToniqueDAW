//! A number in a field, as egui's `DragValue` shows it: dragged sideways
//! (ten times finer with Shift), clicked to take the arrow keys (a step,
//! a big one with Shift), double-clicked to type it (Enter or a click
//! elsewhere takes it, Esc doesn't).

use std::ops::RangeInclusive;

use egui::{
    Align, Align2, CursorIcon, FontId, Key, Response, Sense, TextEdit, Ui, UiBuilder, Widget,
};

use crate::ui::{
    theme::font_size,
    widget::arrow_keys::{arrow_presses, paint_outline},
};
use tonique_core::utils::steps::KeyStep;

/// Shift-dragging moves this much slower.
const FINE_DRAG: f64 = 0.1;

/// Reads a typed value.
type Parser<'a> = Box<dyn Fn(&str) -> Option<f64> + 'a>;

/// What's typed into it, and whether it has had the focus yet.
#[derive(Clone)]
struct Typing {
    text: String,
    focused: bool,
}

/// The response is marked changed when the value is: while dragged, or
/// at once (stepped by a key, typed).
pub struct StepField<'a> {
    value: &'a mut f64,
    /// Per point dragged.
    speed: f64,
    step: KeyStep,
    range: RangeInclusive<f64>,
    format: Box<dyn Fn(f64) -> String + 'a>,
    parse: Parser<'a>,
}

impl<'a> StepField<'a> {
    pub fn new(value: &'a mut f64, step: KeyStep) -> Self {
        Self {
            value,
            speed: step.small,
            step,
            range: f64::MIN..=f64::MAX,
            format: Box::new(|value| format!("{value:.2}")),
            parse: Box::new(|text| text.trim().parse().ok()),
        }
    }

    pub fn speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }

    pub fn range(mut self, range: RangeInclusive<f64>) -> Self {
        self.range = range;
        self
    }

    pub fn format(mut self, format: impl Fn(f64) -> String + 'a) -> Self {
        self.format = Box::new(format);
        self
    }

    pub fn parse(mut self, parse: impl Fn(&str) -> Option<f64> + 'a) -> Self {
        self.parse = Box::new(parse);
        self
    }

    fn clamp(&self, value: f64) -> f64 {
        value.clamp(*self.range.start(), *self.range.end())
    }

    /// Typing into it: the text's field over it.
    fn typing_ui(self, ui: &mut Ui, mut response: Response, mut typing: Typing) -> Response {
        let typing_id = response.id.with("typing");
        let rect = response.rect;
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
        let edit = TextEdit::singleline(&mut typing.text)
            .desired_width(rect.width())
            .horizontal_align(Align::Center)
            .vertical_align(Align::Center)
            .min_size(rect.size())
            .font(field_font(ui))
            .show(&mut child)
            .response;
        if !typing.focused {
            edit.request_focus();
            typing.focused = true;
        } else if !edit.has_focus() {
            // Enter or a click elsewhere takes it; Esc drops it.
            ui.data_mut(|data| data.remove::<Typing>(typing_id));
            if !ui.input(|input| input.key_pressed(Key::Escape))
                && let Some(typed) = (self.parse)(&typing.text)
            {
                let typed = self.clamp(typed);
                if typed != *self.value {
                    *self.value = typed;
                    response.mark_changed();
                }
            }
            return response;
        }
        ui.data_mut(|data| data.insert_temp(typing_id, typing));
        response
    }
}

fn field_font(ui: &Ui) -> FontId {
    ui.style()
        .override_font_id
        .clone()
        .unwrap_or(FontId::proportional(font_size::LABEL))
}

impl Widget for StepField<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let size = ui.spacing().interact_size;
        let (rect, mut response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let typing_id = response.id.with("typing");
        if let Some(typing) = ui.data(|data| data.get_temp::<Typing>(typing_id)) {
            return self.typing_ui(ui, response, typing);
        }
        if response.double_clicked() {
            let text = (self.format)(*self.value);
            ui.data_mut(|data| {
                data.insert_temp(
                    typing_id,
                    Typing {
                        text,
                        focused: false,
                    },
                )
            });
            ui.ctx().request_repaint();
        }

        let before = *self.value;
        if response.dragged() {
            let fine = ui.input(|input| input.modifiers.shift);
            let speed = if fine { FINE_DRAG } else { 1. } * self.speed;
            *self.value = self.clamp(*self.value + response.drag_delta().x as f64 * speed);
        }
        let presses = arrow_presses(ui, &response);
        if !presses.is_empty() && !response.dragged() {
            *self.value = self.clamp(*self.value + presses.amount(self.step));
        }
        if *self.value != before {
            response.mark_changed();
        }
        let response = response.on_hover_and_drag_cursor(CursorIcon::ResizeHorizontal);

        let visuals = ui.style().interact(&response);
        let radius = visuals.corner_radius;
        let painter = ui.painter();
        painter.rect(
            rect,
            radius,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        if response.has_focus() {
            paint_outline(ui, rect, radius.nw as f32);
        }
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            (self.format)(*self.value),
            field_font(ui),
            visuals.text_color(),
        );
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};

    /// Frames of a field at (0, 0) showing `value`, one per batch of
    /// events: whether each changed it.
    fn run(value: &mut f64, frames: Vec<Vec<Event>>) -> Vec<bool> {
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let mut changes = Vec::new();
        for (index, events) in frames.into_iter().enumerate() {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400., 100.))),
                time: Some(index as f64 / 10.),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let field = StepField::new(value, KeyStep::new(0.25, 4.)).range(0.0..=10.);
                changes.push(ui.add(field).changed());
            });
            output.textures_delta.clear();
        }
        changes
    }

    fn click() -> Vec<Event> {
        let at = pos2(10., 5.);
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        vec![Event::PointerMoved(at), button(true), button(false)]
    }

    fn key(key: Key, modifiers: Modifiers) -> Vec<Event> {
        vec![Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }]
    }

    #[test]
    fn clicked_it_steps_without_typing() {
        let mut value = 1.;
        let changes = run(
            &mut value,
            vec![
                vec![],
                key(Key::ArrowUp, Modifiers::NONE),
                click(),
                key(Key::ArrowUp, Modifiers::NONE),
                key(Key::ArrowRight, Modifiers::SHIFT),
                key(Key::ArrowUp, Modifiers::SHIFT),
            ],
        );
        assert_eq!(value, 9.25);
        assert!(!changes[1], "not before it's clicked");
        assert!(!changes[2], "a click doesn't change it");
        assert!(changes[3] && changes[4]);
        run(
            &mut value,
            vec![vec![], click(), key(Key::ArrowUp, Modifiers::SHIFT)],
        );
        assert_eq!(value, 10., "no further than its end");
    }

    #[test]
    fn double_clicked_it_types() {
        let mut value = 1.;
        let mut frames = vec![vec![], click(), click(), vec![]];
        frames.push(key(Key::A, Modifiers::COMMAND));
        frames.push(vec![Event::Text("3.5".into())]);
        frames.push(key(Key::Enter, Modifiers::NONE));
        frames.push(vec![]);
        let changes = run(&mut value, frames);
        assert_eq!(value, 3.5);
        assert!(changes.contains(&true));
    }
}
