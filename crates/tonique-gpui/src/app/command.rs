//! The keymap's commands as a gpui action: a shortcut found in the user's
//! keymap, or a menu item, dispatches `Command(action)` from the focused
//! element up to the window's root, so the view with the keyboard runs it
//! first (the bottom panel deletes its effect, the timeline its clips),
//! and the window runs the rest.
//!
//! Shortcuts are still matched by the keymap (`Keymap::action_for`), as in
//! the egui app, rather than bound as gpui key bindings: gpui runs bindings
//! before any key listener, and fields, the shortcut being recorded and the
//! browser's list take their keys first.

use gpui::{App, Window};

use tonique_core::config::keymap::Action;

#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = tonique, no_json)]
pub struct Command(pub Action);

/// Runs `action` from wherever the keyboard is.
pub fn dispatch(action: Action, window: &mut Window, context: &mut App) {
    window.dispatch_action(Box::new(Command(action)), context);
}
