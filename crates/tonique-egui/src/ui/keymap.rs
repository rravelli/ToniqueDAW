//! The keymap (`config::keymap`) as egui sees it: its keys and modifiers
//! mapped to egui's, the actions this frame's key presses trigger, and
//! shortcuts written as menus show them.

use egui::{Context, Event, InputState, KeyboardShortcut, Modifiers};

use crate::config::keymap::{Action, Key, Keymap, Mods, Shortcut};

/// egui's key with the same name: every key has one (egui's names are the
/// keymap's).
pub fn to_egui_key(key: Key) -> Option<egui::Key> {
    egui::Key::from_name(key.name())
}

pub fn from_egui_key(key: egui::Key) -> Option<Key> {
    Key::from_name(key.name())
}

pub fn to_egui(shortcut: &Shortcut) -> Option<KeyboardShortcut> {
    let Mods {
        command,
        alt,
        shift,
    } = shortcut.mods;
    let mut modifiers = Modifiers::NONE;
    if command {
        modifiers |= Modifiers::COMMAND;
    }
    if alt {
        modifiers |= Modifiers::ALT;
    }
    if shift {
        modifiers |= Modifiers::SHIFT;
    }
    Some(KeyboardShortcut::new(modifiers, to_egui_key(shortcut.key)?))
}

/// `shortcut` as egui writes it for this platform: `Ctrl+Z`, or `⌘Z` on
/// macOS.
pub fn format(ctx: &Context, shortcut: &Shortcut) -> String {
    match to_egui(shortcut) {
        Some(shortcut) => ctx.format_shortcut(&shortcut),
        None => crate::config::keymap::format_shortcut(shortcut),
    }
}

/// The modifiers of a key press, as shortcuts match them: Ctrl (Cmd on
/// macOS) is `command`.
fn mods_of(modifiers: Modifiers) -> Mods {
    Mods {
        command: modifiers.command,
        alt: modifiers.alt,
        shift: modifiers.shift,
    }
}

/// The shortcut for `key` pressed with `modifiers`, in the form bindings
/// are stored: Ctrl and Cmd both become `command`.
pub fn shortcut_from_press(key: egui::Key, modifiers: Modifiers) -> Option<Shortcut> {
    let mods = Mods {
        command: modifiers.command || modifiers.ctrl || modifiers.mac_cmd,
        alt: modifiers.alt,
        shift: modifiers.shift,
    };
    Some(Shortcut::new(mods, from_egui_key(key)?))
}

/// What the keymap does with egui's input and text.
pub trait KeymapExt {
    /// First binding of `action` as shown in menus, e.g. `Ctrl+Z` (`⌘Z` on
    /// macOS).
    fn shortcut_text(&self, ctx: &Context, action: Action) -> String;
    /// `text` followed by the first binding of `action`, for tooltips and
    /// menu labels: `Loop (Ctrl+L)`.
    fn with_shortcut(&self, ctx: &Context, text: &str, action: Action) -> String;
    /// Actions triggered by this frame's key presses, in the order pressed.
    fn triggered(&self, input: &InputState) -> Vec<Action>;
}

impl KeymapExt for Keymap {
    fn shortcut_text(&self, ctx: &Context, action: Action) -> String {
        self.bindings(action)
            .first()
            .map(|shortcut| format(ctx, shortcut))
            .unwrap_or_default()
    }

    fn with_shortcut(&self, ctx: &Context, text: &str, action: Action) -> String {
        match self.shortcut_text(ctx, action).as_str() {
            "" => text.to_string(),
            shortcut => format!("{text} ({shortcut})"),
        }
    }

    fn triggered(&self, input: &InputState) -> Vec<Action> {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Key {
                    key,
                    pressed: true,
                    repeat,
                    modifiers,
                    ..
                } => self
                    .action_for(from_egui_key(*key)?, mods_of(*modifiers))
                    .filter(|action| !repeat || action.repeats()),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::keymap::reserved_by;

    #[test]
    fn every_key_has_an_egui_key() {
        for &key in Key::ALL {
            let egui_key = to_egui_key(key).unwrap_or_else(|| panic!("{key:?}"));
            assert_eq!(from_egui_key(egui_key), Some(key));
        }
    }

    #[test]
    fn presses_normalize_to_command() {
        let ctrl = Modifiers::CTRL | Modifiers::COMMAND;
        let mac_cmd = Modifiers::MAC_CMD | Modifiers::COMMAND;
        let expected = Shortcut::new(Mods::COMMAND.with(Mods::SHIFT), Key::Z);
        let z = egui::Key::Z;
        assert_eq!(
            shortcut_from_press(z, ctrl | Modifiers::SHIFT),
            Some(expected)
        );
        assert_eq!(
            shortcut_from_press(z, mac_cmd | Modifiers::SHIFT),
            Some(expected)
        );
        let c = egui::Key::C;
        assert!(reserved_by(&shortcut_from_press(c, ctrl).unwrap()).is_some());
        assert!(reserved_by(&shortcut_from_press(c, Modifiers::NONE).unwrap()).is_none());
    }

    /// egui writes the bindings as the keymap saves them.
    #[test]
    fn egui_formats_like_the_keymap() {
        for action in Action::ALL {
            for shortcut in action.default_bindings() {
                let egui = to_egui(&shortcut).unwrap();
                assert_eq!(
                    egui.format(&egui::ModifierNames::NAMES, false),
                    crate::config::keymap::format_shortcut(&shortcut)
                );
            }
        }
    }
}
