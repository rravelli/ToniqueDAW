//! Keyboard shortcuts: Tonique's keymap (the user's, from its settings)
//! turned into actions, run as the egui app runs them.

use crate::model::HoldsProject;
use gpui::{App, Context, KeyDownEvent, Keystroke, Window};
use tonique_engine::time::BeatPos;

use crate::{
    app::{
        Tonique,
        command::{self, Command},
        dialog::Dialog,
        project::ProjectAction,
    },
    config::keymap::{Action, Key, Mods, format_shortcut},
};

/// The keymap's key and modifiers for a keystroke.
fn shortcut(keystroke: &Keystroke) -> Option<(Key, Mods)> {
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

impl Tonique {
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
        // A shortcut being recorded in settings takes every key, Esc
        // included, so nothing else runs.
        if self.records_shortcut() {
            if !event.is_held
                && let Some((key, modifiers)) = shortcut(keystroke)
            {
                self.record_shortcut(key, modifiers, context);
            }
            context.notify();
            return;
        }
        // A clicked value control takes the arrows.
        if self.arrow_key(keystroke, window, context) {
            context.notify();
            return;
        }
        match keystroke.key.as_str() {
            "escape" if self.dialog.is_some() => self.close_dialog(window, context),
            "enter" if self.dialog == Some(Dialog::Unsaved) => {
                self.answer_unsaved(true, window, context);
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
                state.paste();
                drop(state);
                self.reveal_selection(context);
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

    /// Runs `action`, as the egui app's `dispatch` and its timeline do.
    fn run(&mut self, action: Action, window: &mut Window, context: &mut Context<Self>) {
        if self.run_on_state(action, context) {
            return;
        }
        match action {
            Action::PlayStop => self.toggle_play(context),
            Action::ToggleBrowser => self.browser_open = !self.browser_open,
            Action::ToggleTrackList => self.headers.toggle_list(),
            // Nudge the selected clips by a grid step, or move the edit
            // cursor when nothing is selected.
            Action::NudgeLeft | Action::NudgeRight => {
                let direction = if action == Action::NudgeLeft { -1. } else { 1. };
                let mut state = self.state_mut(context);
                let step = direction * state.grid.snap_step();
                if state.selected_clips().is_empty() {
                    let cursor = state.grid.snap_to_step(state.edit_cursor() + step);
                    state.set_edit_cursor(cursor);
                    drop(state);
                    self.reveal((cursor, cursor), context);
                } else {
                    state.nudge_selection(step);
                    drop(state);
                    self.reveal_selection(context);
                }
            }
            Action::Duplicate => {
                let mut state = self.state_mut(context);
                if !state.selected_clips().is_empty() {
                    state.duplicate_selected_clips();
                    drop(state);
                    self.reveal_selection(context);
                } else if let Some(selected) = state.selected_track() {
                    state.duplicate_track(&selected.id);
                }
            }
            Action::Rename => {
                if let Some(track) = self.state(context).selected_track() {
                    self.rename_track(track.id, window, context);
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

    /// Runs `action` when it only changes the project; whether it did.
    fn run_on_state(&mut self, action: Action, context: &mut App) -> bool {
        let mut state = self.state_mut(context);
        match action {
            // Loop the selection, or toggle looping.
            Action::Loop => {
                if !state.loop_selection() {
                    let looping = state.looping();
                    state.set_looping(!looping);
                }
            }
            Action::ToggleMetronome => state.toggle_metronome(),
            Action::ToggleFollowPlayhead => {
                let follow = state.follow_playhead();
                state.set_follow_playhead(!follow);
            }
            Action::NarrowGrid => state.grid.narrow(),
            Action::WidenGrid => state.grid.widen(),
            Action::ToggleTripletGrid => state.grid.settings.triplet ^= true,
            Action::ToggleAdaptiveGrid => state.grid.toggle_adaptive(),
            Action::ToggleSnap => state.grid.settings.snap ^= true,
            Action::Undo => state.undo(),
            Action::Redo => state.redo(),
            Action::AddTrack => {
                state.add_track_below_selection();
            }
            Action::SelectAll => state.select_all_clips(),
            Action::MoveTrackUp => state.move_selection_tracks(-1),
            Action::MoveTrackDown => state.move_selection_tracks(1),
            Action::Delete => {
                if !state.selected_clips().is_empty() {
                    state.delete_selected_clips();
                } else if let Some(selected) = state.selected_track() {
                    state.delete_track(&selected.id);
                }
            }
            Action::SplitAtCursor => {
                let cursor = state.edit_cursor();
                for id in state.selected_tracks().clone() {
                    state.cut_clip_at(&id, cursor);
                }
            }
            Action::GroupTracks => {
                let selected = state.selected_tracks().clone();
                state.group(&selected);
            }
            _ => return false,
        }
        true
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
