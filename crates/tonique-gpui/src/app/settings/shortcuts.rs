//! The Shortcuts tab, as the egui app's: every action with its bindings.
//! Clicking a binding records the next key press in its place, right
//! clicking removes it; changes apply at once and are saved with the
//! settings, so both apps share them. Shortcuts the app handles itself
//! (copy, paste, zoom) are listed muted and can't be changed.

use gpui::{Context, MouseButton, ScrollHandle, Window, div, prelude::*, px};

use crate::{
    app::{
        Tonique,
        settings::{ROW_GAP, button_row, section_header, status},
    },
    config::keymap::{
        Action, FIXED_SHORTCUTS, Key, Keymap, Mods, Shortcut, format_shortcut, reserved_by,
    },
    icons,
    theme::{Theme, hsla},
    widgets::{Flat, ScrollbarGrab, dialog_button, glyph, handle_scrollbar, tooltip},
};

const GROUPS: [&str; 5] = ["File", "Transport", "Edit", "Grid", "View"];
/// Fits the longest action name, wider than the other tabs' labels.
const LABEL_WIDTH: f32 = 200.;
const CHIP_HEIGHT: f32 = 20.;
const CHIP_PADDING: f32 = 8.;
const CHIP_GAP: f32 = 4.;
const ICON_SIZE: f32 = 12.;
/// Room kept for the scrollbar right of the bindings.
const SCROLLBAR_ROOM: f32 = 8.;

/// The binding waiting for a key press.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Recording {
    action: Action,
    /// The binding being replaced; `None` adds one.
    index: Option<usize>,
}

#[derive(Default)]
pub struct ShortcutsForm {
    recording: Option<Recording>,
    /// The outcome of the last change, e.g. a shortcut moved from another
    /// action.
    message: Option<String>,
    scroll: ScrollHandle,
    scrollbar: ScrollbarGrab,
}

impl ShortcutsForm {
    /// Stops recording and forgets the last outcome, e.g. on changing tab.
    pub fn cancel(&mut self) {
        self.recording = None;
        self.message = None;
    }

    /// Stops recording, keeping the last outcome: a press anywhere does,
    /// before the recorder takes it back if it was on it.
    pub fn stop_recording(&mut self) {
        self.recording = None;
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    fn start(&mut self, recording: Recording) {
        self.recording = Some(recording);
        self.message = None;
    }

    /// Binds `shortcut` to what's being recorded, and tells what that
    /// changed. Esc cancels; a fixed shortcut is refused, recording on.
    fn record(&mut self, keymap: &mut Keymap, shortcut: Shortcut) {
        let Some(recording) = self.recording else {
            return;
        };
        if shortcut == Shortcut::new(Mods::NONE, Key::Escape) {
            self.recording = None;
        } else if let Some(fixed) = reserved_by(&shortcut) {
            self.message = Some(format!(
                "{} is reserved for {}.",
                format_shortcut(&shortcut),
                fixed.label
            ));
        } else {
            let previous = keymap.assign(recording.action, recording.index, shortcut);
            self.message = previous.map(|other| {
                format!(
                    "{} was removed from “{}”.",
                    format_shortcut(&shortcut),
                    other.label()
                )
            });
            self.recording = None;
        }
    }
}

impl Tonique {
    /// Whether a key press goes to the shortcut being recorded.
    pub(in crate::app) fn records_shortcut(&self) -> bool {
        self.dialog == Some(crate::app::dialog::Dialog::Settings)
            && self.settings.shortcuts.is_recording()
    }

    /// Binds the shortcut pressed, `key` with `modifiers`, to what's being
    /// recorded.
    pub(in crate::app) fn record_shortcut(&mut self, key: Key, modifiers: Mods) {
        let mut keymap = self.state.settings().keymap.clone();
        self.settings
            .shortcuts
            .record(&mut keymap, Shortcut::new(modifiers, key));
        self.set_keymap(keymap);
    }

    /// Applies and saves `keymap`, when it changed.
    fn set_keymap(&mut self, keymap: Keymap) {
        if keymap == self.state.settings().keymap {
            return;
        }
        let mut settings = self.state.settings().clone();
        settings.keymap = keymap.clone();
        self.settings.draft.keymap = keymap;
        self.state.apply_settings(settings);
    }

    /// Changes the keymap in use by `change`, and stops recording.
    fn edit_keymap(&mut self, change: impl FnOnce(&mut Keymap)) {
        let mut keymap = self.state.settings().keymap.clone();
        change(&mut keymap);
        self.settings.shortcuts.cancel();
        self.set_keymap(keymap);
    }

    /// Records the next key press as `recording`. The app's own focus
    /// takes the keyboard, so a field typed in doesn't get it.
    fn start_recording(
        &mut self,
        recording: Recording,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        self.settings.shortcuts.start(recording);
        window.focus(&self.focus, context);
    }

    pub(super) fn shortcuts_tab(&self, theme: &Theme, context: &mut Context<Self>) -> gpui::Div {
        let keymap = &self.state.settings().keymap;
        let groups = GROUPS.into_iter().map(|group| {
            let actions = Action::ALL
                .into_iter()
                .filter(|action| action.is_available() && action.group() == group)
                .map(|action| self.action_row(action, keymap, theme, context));
            let fixed = FIXED_SHORTCUTS
                .iter()
                .filter(|fixed| fixed.group == group)
                .map(|fixed| {
                    let chips = fixed.shortcuts.iter().enumerate().map(|(index, shortcut)| {
                        Flat::new((fixed.label, index), CHIP_HEIGHT)
                            .fit(CHIP_PADDING)
                            .enabled(false)
                            .element(format_shortcut(shortcut), theme)
                    });
                    shortcut_row(fixed.label, chips, true, theme)
                });
            div()
                .flex()
                .flex_col()
                .gap(px(ROW_GAP))
                .child(section_header(group, theme))
                .children(actions)
                .children(fixed)
        });
        let list = div()
            .id("shortcut-list")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.settings.shortcuts.scroll)
            .pr(px(SCROLLBAR_ROOM))
            .flex()
            .flex_col()
            .gap(px(ROW_GAP * 2.))
            .children(groups);
        let list = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(list)
            .child(handle_scrollbar(
                &self.settings.shortcuts.scroll,
                &self.settings.shortcuts.scrollbar,
                0.,
                theme,
                context,
            ));

        let form = &self.settings.shortcuts;
        let hint = if form.is_recording() {
            "Press the new shortcut, or Esc to cancel."
        } else {
            "Click a shortcut to change it, right-click to remove it."
        };
        let message = form.message.clone().unwrap_or_else(|| hint.to_string());
        let all_default = keymap.is_all_default();
        let reset_all = dialog_button("reset-shortcuts", "Reset all", false, theme);
        let reset_all = if all_default {
            reset_all.opacity(0.5)
        } else {
            reset_all.on_mouse_down(
                MouseButton::Left,
                context.listener(|this, _, _, context| {
                    this.edit_keymap(|keymap| *keymap = Keymap::default());
                    context.notify();
                }),
            )
        };
        div()
            .h_full()
            .child(list)
            .child(status(message, theme).flex_none())
            .child(button_row(theme).flex_none().justify_end().child(reset_all))
    }

    /// `action`'s name, its bindings as chips (the one being recorded
    /// waiting for a key), then buttons to add one and restore the
    /// default ones.
    fn action_row(
        &self,
        action: Action,
        keymap: &Keymap,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let recording = self.settings.shortcuts.recording;
        let id = action.id();
        let mut chips: Vec<gpui::AnyElement> = Vec::new();
        for (index, shortcut) in keymap.bindings(action).iter().enumerate() {
            let replacing = Recording {
                action,
                index: Some(index),
            };
            if recording == Some(replacing) {
                chips.push(self.recorder(replacing, theme, context).into_any_element());
                continue;
            }
            let chip = Flat::new((id, index), CHIP_HEIGHT)
                .fit(CHIP_PADDING)
                .element(format_shortcut(shortcut), theme)
                .on_mouse_down(
                    MouseButton::Left,
                    context.listener(move |this, _, window, context| {
                        this.start_recording(replacing, window, context);
                        context.notify();
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    context.listener(move |this, _, _, context| {
                        this.edit_keymap(|keymap| keymap.remove(action, index));
                        context.notify();
                    }),
                );
            chips.push(chip.into_any_element());
        }

        let adding = Recording {
            action,
            index: None,
        };
        if recording == Some(adding) {
            chips.push(self.recorder(adding, theme, context).into_any_element());
        } else {
            let add = icon_button((id, usize::MAX - 1), icons::PLUS, theme)
                .tooltip(tooltip("Add a shortcut", theme))
                .on_mouse_down(
                    MouseButton::Left,
                    context.listener(move |this, _, window, context| {
                        this.start_recording(adding, window, context);
                        context.notify();
                    }),
                );
            chips.push(add.into_any_element());
        }

        if !keymap.is_default(action) {
            let reset = icon_button((id, usize::MAX), icons::ARROW_COUNTER_CLOCKWISE, theme)
                .tooltip(tooltip("Restore the default", theme))
                .on_mouse_down(
                    MouseButton::Left,
                    context.listener(move |this, _, _, context| {
                        this.edit_keymap(|keymap| keymap.reset(action));
                        context.notify();
                    }),
                );
            chips.push(reset.into_any_element());
        }
        shortcut_row(action.label(), chips, false, theme)
    }

    /// The chip waiting for a key press. Pressing it keeps recording: the
    /// press first stops it, as any press does.
    fn recorder(
        &self,
        recording: Recording,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        Flat::new("shortcut-recorder", CHIP_HEIGHT)
            .fit(CHIP_PADDING)
            .selected(true)
            .element("Press a key…", theme)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, _, window, context| {
                    this.start_recording(recording, window, context);
                    context.notify();
                }),
            )
    }
}

/// A shortcut's name, muted for a `fixed` one, then its `chips`.
fn shortcut_row(
    label: &'static str,
    chips: impl IntoIterator<Item = impl IntoElement>,
    fixed: bool,
    theme: &Theme,
) -> gpui::Div {
    let color = if fixed { theme.text_muted } else { theme.text };
    div()
        .flex()
        .items_center()
        .min_h(px(CHIP_HEIGHT))
        .child(
            div()
                .w(px(LABEL_WIDTH))
                .flex_none()
                .text_color(hsla(color))
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(CHIP_GAP))
                .children(chips),
        )
}

fn icon_button(
    id: impl Into<gpui::ElementId>,
    icon: &'static str,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    Flat::ghost(id, CHIP_HEIGHT).element(glyph(icon, ICON_SIZE, false), theme)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shortcut(mods: Mods, key: Key) -> Shortcut {
        Shortcut::new(mods, key)
    }

    #[test]
    fn a_press_replaces_the_binding_recorded() {
        let mut form = ShortcutsForm::default();
        let mut keymap = Keymap::default();
        form.start(Recording {
            action: Action::Loop,
            index: Some(0),
        });
        let pressed = shortcut(Mods::COMMAND.with(Mods::SHIFT), Key::F9);
        form.record(&mut keymap, pressed);
        assert_eq!(keymap.bindings(Action::Loop).first(), Some(&pressed));
        assert!(!form.is_recording());
        assert_eq!(form.message, None);
    }

    #[test]
    fn a_press_taken_from_another_action_says_so() {
        let mut form = ShortcutsForm::default();
        let mut keymap = Keymap::default();
        let taken = keymap.bindings(Action::Undo)[0];
        form.start(Recording {
            action: Action::Loop,
            index: None,
        });
        form.record(&mut keymap, taken);
        assert!(keymap.bindings(Action::Loop).contains(&taken));
        assert!(!keymap.bindings(Action::Undo).contains(&taken));
        let message = form.message.unwrap();
        assert!(message.contains(Action::Undo.label()), "{message}");
    }

    #[test]
    fn escape_cancels_and_fixed_shortcuts_are_refused() {
        let mut form = ShortcutsForm::default();
        let mut keymap = Keymap::default();
        let recording = Recording {
            action: Action::Loop,
            index: None,
        };
        form.start(recording);
        form.record(&mut keymap, shortcut(Mods::COMMAND, Key::C));
        assert!(form.is_recording());
        assert!(form.message.as_deref().unwrap().contains("Copy"));
        form.record(&mut keymap, shortcut(Mods::NONE, Key::Escape));
        assert!(!form.is_recording());
        assert_eq!(keymap, Keymap::default());
    }
}
