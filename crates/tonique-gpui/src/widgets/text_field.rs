//! One line of text typed at its end: for searching, renaming and number
//! fields. The caller owns it and passes it its keys.

use gpui::{
    Context, FocusHandle, KeyDownEvent, Keystroke, MouseButton, Window, div, prelude::*, px,
};

use crate::{
    app::Tonique,
    theme::{Theme, hsla},
};

/// What a key did to a field.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FieldKey {
    /// The text changed.
    Edited,
    /// Enter.
    Submit,
    /// Escape.
    Cancel,
    /// Not the field's: the caller may use it.
    Ignored,
}

/// `keystroke` typed at the end of `text`.
fn edit(text: &mut String, keystroke: &Keystroke) -> FieldKey {
    let command = keystroke.modifiers.control || keystroke.modifiers.platform;
    match keystroke.key.as_str() {
        "backspace" if command => text.clear(),
        "backspace" => {
            text.pop();
        }
        "enter" => return FieldKey::Submit,
        "escape" => return FieldKey::Cancel,
        _ if command => return FieldKey::Ignored,
        _ => match &keystroke.key_char {
            Some(typed) if !typed.chars().any(char::is_control) => text.push_str(typed),
            _ => return FieldKey::Ignored,
        },
    }
    FieldKey::Edited
}

pub struct TextField {
    pub text: String,
    pub focus: FocusHandle,
}

impl TextField {
    pub fn new(context: &mut Context<Tonique>) -> Self {
        Self {
            text: String::new(),
            focus: context.focus_handle(),
        }
    }

    /// Typing `text` from now, the field taking the keyboard.
    pub fn start(&mut self, text: String, window: &mut Window, context: &mut Context<Tonique>) {
        self.text = text;
        window.focus(&self.focus, context);
    }

    pub fn key(&mut self, event: &KeyDownEvent) -> FieldKey {
        edit(&mut self.text, &event.keystroke)
    }

    /// The text and a caret, for a field that has the keyboard.
    pub fn view_focused(&self) -> gpui::Div {
        div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .child(format!("{}▏", self.text))
    }

    /// The text, a caret after it while focused, or `placeholder` dimmed
    /// while empty and not.
    pub fn view(&self, placeholder: &str, theme: &Theme, window: &Window) -> gpui::Div {
        let focused = self.focus.is_focused(window);
        let element = div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap();
        if self.text.is_empty() && !focused {
            return element
                .text_color(hsla(theme.text_disabled))
                .child(placeholder.to_string());
        }
        element.child(format!("{}{}", self.text, if focused { "▏" } else { "" }))
    }
}

/// A line being typed, with the keyboard: Enter or a click elsewhere
/// `finish`es it keeping the text, Escape without. `get` finds its field.
pub fn typing_view(
    id: &'static str,
    field: &TextField,
    theme: &Theme,
    context: &mut Context<Tonique>,
    get: fn(&mut Tonique) -> Option<&mut TextField>,
    finish: fn(&mut Tonique, bool, &mut Window, &mut Context<Tonique>),
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .track_focus(&field.focus)
        .flex()
        .items_center()
        .size_full()
        .px(px(2.))
        .rounded(px(1.))
        .bg(hsla(theme.bg_deep))
        .text_color(hsla(theme.text))
        .on_key_down(
            context.listener(move |this, event: &KeyDownEvent, window, context| {
                let Some(field) = get(this) else {
                    return;
                };
                match field.key(event) {
                    FieldKey::Submit => finish(this, true, window, context),
                    FieldKey::Cancel => finish(this, false, window, context),
                    FieldKey::Edited => {}
                    FieldKey::Ignored => return,
                }
                context.stop_propagation();
                context.notify();
            }),
        )
        .on_mouse_down(MouseButton::Left, |_, _, context| {
            context.stop_propagation()
        })
        .on_mouse_down_out(context.listener(move |this, _, window, context| {
            finish(this, true, window, context);
            context.notify();
        }))
        .child(field.view_focused())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(text: &mut String, keystroke: &'static str) -> FieldKey {
        let mut pressed = Keystroke::parse(keystroke).unwrap();
        // As typed: a character key carries what it types.
        if pressed.key.chars().count() == 1 && !pressed.modifiers.control {
            pressed.key_char = Some(pressed.key.clone());
        }
        edit(text, &pressed)
    }

    #[test]
    fn typing_erasing_and_leaving() {
        let mut text = String::from("12");
        assert_eq!(press(&mut text, "3"), FieldKey::Edited);
        assert_eq!(press(&mut text, "backspace"), FieldKey::Edited);
        assert_eq!(text, "12");
        assert_eq!(press(&mut text, "ctrl-a"), FieldKey::Ignored);
        assert_eq!(press(&mut text, "ctrl-backspace"), FieldKey::Edited);
        assert_eq!(text, "");
        assert_eq!(press(&mut text, "enter"), FieldKey::Submit);
        assert_eq!(press(&mut text, "escape"), FieldKey::Cancel);
    }
}
