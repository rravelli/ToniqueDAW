//! Keyboard shortcuts: Tonique's keymap (the user's, from its settings)
//! turned into actions, run as the egui app runs them.

use gpui::{Context, KeyDownEvent, Keystroke, Window};
use tonique_engine::time::BeatPos;

use crate::{
    app::{Tonique, dialog::Dialog, project::ProjectAction},
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
    pub(super) fn shortcut_text(&self, action: Action) -> String {
        let bindings = self.state.settings().keymap.bindings(action);
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
                self.record_shortcut(key, modifiers);
            }
            context.notify();
            return;
        }
        // The browser's list takes the arrows and Enter while focused.
        if self.browser.list_focus.is_focused(window) && self.browser_key(event, context) {
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
            "escape" if self.popup.is_some() => self.popup = None,
            "escape" if self.arrows.is_some() => self.release_arrows(),
            "escape" => self.state.clear_clip_selection(),
            "home" => self.state.seek(BeatPos::ZERO),
            "c" | "x" | "v" if is_clipboard(keystroke) && self.has_keyboard(window, context) => {
                self.clipboard(keystroke.key.as_str());
            }
            _ => {
                let keymap = &self.state.settings().keymap;
                let Some(action) = shortcut(keystroke)
                    .and_then(|(key, modifiers)| keymap.action_for(key, modifiers))
                else {
                    return;
                };
                if !self.bottom_action(action, window, context) {
                    self.run(action, window, context);
                }
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
    fn clipboard(&mut self, key: &str) {
        let state = &mut self.state;
        match key {
            "c" => {
                state.copy_selection();
            }
            "x" => state.cut_selection(),
            _ => {
                state.paste();
                self.reveal_selection();
            }
        }
    }

    /// Runs `action`, as the egui app's `dispatch` and its timeline do.
    /// Those of parts this app doesn't have (the graph) do nothing.
    fn run(&mut self, action: Action, window: &mut Window, context: &mut Context<Self>) {
        let state = &mut self.state;
        match action {
            Action::PlayStop => self.toggle_play(),
            // Loop the selection, or toggle looping.
            Action::Loop => {
                if !state.loop_selection() {
                    state.set_looping(!state.looping());
                }
            }
            Action::ToggleMetronome => state.toggle_metronome(),
            Action::ToggleFollowPlayhead => state.set_follow_playhead(!state.follow_playhead()),
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
            Action::ToggleBrowser => self.browser_open = !self.browser_open,
            Action::ToggleTrackList => self.headers.toggle_list(),
            Action::SelectAll => state.select_all_clips(),
            // Nudge the selected clips by a grid step, or move the edit
            // cursor when nothing is selected.
            Action::NudgeLeft | Action::NudgeRight => {
                let direction = if action == Action::NudgeLeft { -1. } else { 1. };
                let step = direction * state.grid.snap_step();
                if state.selected_clips().is_empty() {
                    let cursor = state.grid.snap_to_step(state.edit_cursor() + step);
                    state.set_edit_cursor(cursor);
                    self.reveal((cursor, cursor));
                } else {
                    state.nudge_selection(step);
                    self.reveal_selection();
                }
            }
            Action::MoveTrackUp => state.move_selection_tracks(-1),
            Action::MoveTrackDown => state.move_selection_tracks(1),
            Action::Duplicate => {
                if !state.selected_clips().is_empty() {
                    state.duplicate_selected_clips();
                    self.reveal_selection();
                } else if let Some(selected) = state.selected_track() {
                    state.duplicate_track(&selected.id);
                }
            }
            Action::Delete => {
                if !state.selected_clips().is_empty() {
                    state.delete_selected_clips();
                } else if let Some(selected) = state.selected_track() {
                    state.delete_track(&selected.id);
                }
            }
            Action::SplitAtCursor => {
                for id in state.selected_tracks().clone() {
                    state.cut_clip_at(&id, state.edit_cursor());
                }
            }
            Action::Rename => {
                if let Some(track) = state.selected_track() {
                    self.rename_track(track.id, window, context);
                }
            }
            Action::NewProject => self.project_action(ProjectAction::New, window, context),
            Action::OpenProject => self.project_action(ProjectAction::Open, window, context),
            Action::SaveProject => self.project_action(ProjectAction::Save, window, context),
            Action::SaveProjectAs => self.project_action(ProjectAction::SaveAs, window, context),
            Action::OpenSettings => self.toggle_settings(),
            Action::ToggleEffectsPanel => self.bottom.toggle(),
            Action::GroupTracks => {
                let selected = state.selected_tracks().clone();
                state.group(&selected);
            }
            Action::ExportAudio => self.open_export(window, context),
            Action::ToggleGraphView => {}
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
