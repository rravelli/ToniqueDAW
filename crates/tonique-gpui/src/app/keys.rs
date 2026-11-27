//! Keyboard shortcuts: Tonique's keymap (the user's, from its settings)
//! turned into actions, run by the core when they only change the project.

use crate::model::HoldsProject;
use gpui::{App, Context, KeyDownEvent, Keystroke, Window};
use tonique_engine::time::BeatPos;

use crate::app::{
    Workspace,
    command::{self, Command},
    dialog::Dialog,
    project::ProjectAction,
};
use tonique_core::{
    config::keymap::{Action, Key, Mods, format_shortcut},
    project::ActionOutcome,
};

/// The keymap's key and modifiers for a keystroke.
pub fn shortcut(keystroke: &Keystroke) -> Option<(Key, Mods)> {
    // gpui names keys in lower case: `space`, `left`, `a`.
    let mut name = keystroke.key.clone();
    if let Some(first) = name.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    let key = Key::from_name(&keystroke.key).or_else(|| Key::from_name(&name))?;
    let modifiers = Mods {
        command: keystroke.modifiers.control || keystroke.modifiers.platform,
        alt: keystroke.modifiers.alt,
        shift: keystroke.modifiers.shift,
    };
    Some((key, modifiers))
}

/// Ctrl (Cmd) alone with a key: copy, cut and paste, which aren't in the
/// keymap.
fn is_clipboard(keystroke: &Keystroke) -> bool {
    let modifiers = &keystroke.modifiers;
    (modifiers.control || modifiers.platform) && !modifiers.alt && !modifiers.shift
}

impl Workspace {
    /// `action`'s first shortcut, as written in settings (`Ctrl+S`), for
    /// menus; empty when it has none.
    pub(super) fn shortcut_text(&self, action: Action, context: &App) -> String {
        let bindings = self.state(context).settings().keymap.bindings(action);
        bindings.first().map(format_shortcut).unwrap_or_default()
    }

    pub(super) fn on_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        // A clicked value control takes the arrows.
        if self.arrow_key(keystroke, window, context) {
            context.notify();
            return;
        }
        match keystroke.key.as_str() {
            "escape" if self.dialog.is_some() => self.close_dialog(window, context),
            "enter" if self.dialog == Some(Dialog::Unsaved) => {
                self.answer_unsaved(Some(true), window, context);
            }
            "escape" if crate::popup::shown(context).is_some() => crate::popup::close(context),
            "escape" if crate::app::arrows::held(context).is_some() => {
                crate::app::arrows::release(context)
            }
            "escape" => self.state_mut(context).clear_clip_selection(),
            "home" => self.state_mut(context).seek(BeatPos::ZERO),
            "c" | "x" | "v" if is_clipboard(keystroke) && self.has_keyboard(window, context) => {
                self.clipboard(keystroke.key.as_str(), context);
            }
            _ => {
                let keymap = &self.state(context).settings().keymap;
                let Some(action) = shortcut(keystroke)
                    .and_then(|(key, modifiers)| keymap.action_for(key, modifiers))
                else {
                    return;
                };
                command::dispatch(action, window, context);
            }
        }
        context.notify();
    }

    /// Whether the keyboard is the app's, not a field's or a list's.
    pub(super) fn has_keyboard(&self, window: &Window, context: &Context<Self>) -> bool {
        window
            .focused(context)
            .is_none_or(|focused| focused == self.focus)
    }

    /// Ctrl+C, X or V on the clips: copied (cropped to the zone), cut, or
    /// pasted at the edit cursor, revealed.
    fn clipboard(&mut self, key: &str, context: &mut App) {
        let mut state = self.state_mut(context);
        match key {
            "c" => {
                state.copy_selection();
            }
            "x" => state.cut_selection(),
            _ => {
                if let ActionOutcome::Reveal(start, end) = state.paste_and_reveal() {
                    drop(state);
                    self.arrangement.update(context, |arrangement, context| {
                        arrangement.reveal((start, end), context);
                        context.notify();
                    });
                }
            }
        }
    }

    /// Runs a command reaching the window: the bottom panel's first.
    pub(super) fn on_command(
        &mut self,
        Command(action): &Command,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let taken = self.bottom.update(context, |bottom, context| {
            bottom.bottom_action(*action, window, context)
        });
        if !taken {
            self.run(*action, window, context);
        }
        context.notify();
    }

    /// Runs `action`: the project's part in the core, the rest here.
    fn run(&mut self, action: Action, window: &mut Window, context: &mut Context<Self>) {
        let view = *self.timeline_view.read(context);
        let outcome = self.state_mut(context).run_action(action, &view);
        match outcome {
            ActionOutcome::Done => return,
            ActionOutcome::Reveal(start, end) => {
                self.arrangement.update(context, |arrangement, context| {
                    arrangement.reveal((start, end), context);
                    context.notify();
                });
                return;
            }
            ActionOutcome::Unhandled => {}
        }
        match action {
            Action::ToggleBrowser => self.browser_open = !self.browser_open,
            Action::ToggleTrackList => self.arrangement.update(context, |arrangement, context| {
                arrangement.headers.toggle_list();
                context.notify();
            }),
            Action::Rename => {
                if let Some(track) = self.state(context).selected_track() {
                    self.arrangement.update(context, |arrangement, context| {
                        arrangement.rename_track(track.id, window, context);
                    });
                }
            }
            Action::NewProject => self.project_action(ProjectAction::New, window, context),
            Action::OpenProject => self.project_action(ProjectAction::Open, window, context),
            Action::SaveProject => self.project_action(ProjectAction::Save, window, context),
            Action::SaveProjectAs => self.project_action(ProjectAction::SaveAs, window, context),
            Action::OpenSettings => self.toggle_settings(context),
            Action::ToggleEffectsPanel => self.bottom.update(context, |bottom, context| {
                bottom.toggle();
                context.notify();
            }),
            Action::ExportAudio => self.open_export(window, context),
            Action::ToggleGraphView => self.toggle_graph(context),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keystrokes_find_the_keymaps_keys() {
        let key = |text: &'static str| {
            shortcut(&Keystroke::parse(text).unwrap()).map(|(found_key, _)| found_key)
        };
        assert_eq!(key("space"), Some(Key::Space));
        assert_eq!(key("left"), Some(Key::ArrowLeft));
        assert_eq!(key("ctrl-l"), Some(Key::L));
        assert_eq!(key("ctrl-shift-j"), Some(Key::J));
        assert_eq!(key("delete"), Some(Key::Delete));
        assert_eq!(key("ctrl-1"), Some(Key::Num1));
        let (_, modifiers) = shortcut(&Keystroke::parse("ctrl-shift-f").unwrap()).unwrap();
        assert_eq!(modifiers, Mods::COMMAND.with(Mods::SHIFT));
    }
}
