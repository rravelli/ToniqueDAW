//! What the keymap's actions do to the project, the same in every UI: a
//! shortcut, a menu item or a button runs [`ProjectState::run_action`],
//! and the UI does what's left (panels, dialogs, scrolling to the change).

use tonique_engine::time::BeatPos;

use super::{PlaybackState, ProjectState};
use crate::config::keymap::Action;

/// What a UI has to do after [`ProjectState::run_action`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ActionOutcome {
    /// The project did it all.
    Done,
    /// Done; the arrangement should scroll to show this span, in beats
    /// (the clips just moved or copied, the edit cursor moved).
    Reveal(BeatPos, BeatPos),
    /// Not the project's to run: a panel, a dialog, the project's file,
    /// renaming in place.
    Unhandled,
}

impl ProjectState {
    /// Runs `action` when it only changes the project.
    pub fn run_action(&mut self, action: Action) -> ActionOutcome {
        match action {
            Action::PlayStop => self.toggle_play(),
            // Loop the selection, or toggle looping.
            Action::Loop => {
                if !self.loop_selection() {
                    self.set_looping(!self.looping());
                }
            }
            Action::ToggleMetronome => self.toggle_metronome(),
            Action::ToggleFollowPlayhead => self.set_follow_playhead(!self.follow_playhead()),
            Action::NarrowGrid => self.grid.narrow(),
            Action::WidenGrid => self.grid.widen(),
            Action::ToggleTripletGrid => self.grid.settings.triplet ^= true,
            Action::ToggleAdaptiveGrid => self.grid.toggle_adaptive(),
            Action::ToggleSnap => self.grid.settings.snap ^= true,
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::AddTrack => {
                self.add_track_below_selection();
            }
            Action::GroupTracks => {
                let selected = self.selected_tracks().clone();
                self.group(&selected);
            }
            Action::SelectAll => self.select_all_clips(),
            Action::MoveTrackUp => self.move_selection_tracks(-1),
            Action::MoveTrackDown => self.move_selection_tracks(1),
            // Nudge the selected clips by a grid step, or move the edit
            // cursor when nothing is selected.
            Action::NudgeLeft | Action::NudgeRight => {
                let direction = if action == Action::NudgeLeft { -1. } else { 1. };
                let step = direction * self.grid.snap_step();
                if self.selected_clips().is_empty() {
                    let cursor = self.grid.snap_to_step(self.edit_cursor() + step);
                    self.set_edit_cursor(cursor);
                    return ActionOutcome::Reveal(cursor, cursor);
                }
                self.nudge_selection(step);
                return self.reveal_selection();
            }
            // The selected clips, else the selected track.
            Action::Duplicate => {
                if !self.selected_clips().is_empty() {
                    self.duplicate_selected_clips();
                    return self.reveal_selection();
                }
                if let Some(selected) = self.selected_track() {
                    self.duplicate_track(&selected.id);
                }
            }
            Action::Delete => {
                if !self.selected_clips().is_empty() {
                    self.delete_selected_clips();
                } else if let Some(selected) = self.selected_track() {
                    self.delete_track(&selected.id);
                }
            }
            Action::SplitAtCursor => {
                let cursor = self.edit_cursor();
                for id in self.selected_tracks().clone() {
                    self.cut_clip_at(&id, cursor);
                }
            }
            Action::NewProject
            | Action::OpenProject
            | Action::SaveProject
            | Action::SaveProjectAs
            | Action::ExportAudio
            | Action::Rename
            | Action::ToggleBrowser
            | Action::ToggleEffectsPanel
            | Action::ToggleTrackList
            | Action::ToggleGraphView
            | Action::OpenSettings => return ActionOutcome::Unhandled,
        }
        ActionOutcome::Done
    }

    /// Plays from the playhead, or stops.
    pub fn toggle_play(&mut self) {
        if self.playback_state() == PlaybackState::Playing {
            self.stop();
        } else {
            self.play();
        }
    }

    /// Pastes the clipboard at the edit cursor; the span pasted, to show.
    pub fn paste_and_reveal(&mut self) -> ActionOutcome {
        self.paste();
        self.reveal_selection()
    }

    /// The selected clips' span to show, if any.
    fn reveal_selection(&self) -> ActionOutcome {
        match self.selection_range() {
            Some((start, end)) => ActionOutcome::Reveal(start, end),
            None => ActionOutcome::Done,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonique_engine::engine::{Engine, EngineConfig};

    fn state() -> ProjectState {
        let (engine, _processor) = Engine::new(EngineConfig::default());
        ProjectState::new(engine)
    }

    #[test]
    fn ui_actions_are_left_to_the_ui() {
        let mut state = state();
        for action in [Action::Rename, Action::OpenSettings, Action::SaveProject] {
            assert_eq!(state.run_action(action), ActionOutcome::Unhandled);
        }
    }

    #[test]
    fn nudging_without_a_selection_moves_the_edit_cursor_into_view() {
        let mut state = state();
        let step = state.grid.snap_step();
        let outcome = state.run_action(Action::NudgeRight);
        assert_eq!(state.edit_cursor(), BeatPos(step));
        assert_eq!(outcome, ActionOutcome::Reveal(BeatPos(step), BeatPos(step)));
    }

    #[test]
    fn deleting_without_clips_selected_deletes_the_selected_track() {
        let mut state = state();
        let id = state.add_track();
        state.select_track(&id);
        let tracks = state.track_count();
        assert_eq!(state.run_action(Action::Delete), ActionOutcome::Done);
        // Deleting a track waits for the next update.
        state.update();
        assert_eq!(state.track_count(), tracks - 1);
    }
}
