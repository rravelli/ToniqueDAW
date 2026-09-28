use egui::{Event, Rect, Ui};

use crate::{
    config::keymap::Action,
    core::state::ToniqueProjectState,
    ui::view::timeline::{Timeline, scroll::reveal},
};

impl Timeline {
    pub fn handle_key_press(
        &mut self,
        ui: &mut Ui,
        state: &mut ToniqueProjectState,
        viewport: Rect,
    ) {
        // If other element focused do not check
        if ui.memory(|m| m.focused().is_some()) {
            return;
        }

        self.handle_clipboard(ui, state, viewport);

        if !ui.input(|i| i.focused) {
            return;
        }
        // The other actions work in every view: `ToniqueApp` runs them.
        let actions = ui.input(|i| state.settings().keymap.triggered(i));
        for action in actions.into_iter().filter(|a| a.is_timeline()) {
            self.run_action(action, ui, state, viewport);
        }
    }

    fn run_action(
        &mut self,
        action: Action,
        ui: &mut Ui,
        state: &mut ToniqueProjectState,
        viewport: Rect,
    ) {
        match action {
            // Nudge the selected clips by a grid step, or move the edit
            // cursor when nothing is selected.
            Action::NudgeLeft | Action::NudgeRight => {
                let direction = if action == Action::NudgeLeft { -1. } else { 1. };
                let step = direction * state.grid.step_beats();
                if state.selected_clips().is_empty() {
                    let cursor = state.grid.snap_to_step(state.edit_cursor() + step);
                    state.set_edit_cursor(cursor);
                    reveal(ui, state, viewport, (cursor, cursor));
                } else {
                    state.nudge_selection(step);
                    reveal_selection(ui, state, viewport);
                }
            }
            Action::MoveTrackUp => state.move_selection_tracks(-1),
            Action::MoveTrackDown => state.move_selection_tracks(1),
            Action::Duplicate => {
                if !state.selected_clips().is_empty() {
                    state.duplicate_selected_clips();
                    reveal_selection(ui, state, viewport);
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
            Action::SelectAll => state.select_all_clips(),
            _ => {}
        }
    }

    /// Ctrl+C/X/V. eframe reports them as clipboard events rather than
    /// keys, and only reports a paste when the system clipboard holds text,
    /// so copying also puts a line of text there.
    fn handle_clipboard(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState, viewport: Rect) {
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                Event::Copy | Event::Cut => {
                    let count = state.selected_clips().len();
                    if count == 0 {
                        continue;
                    }
                    if matches!(event, Event::Cut) {
                        state.cut_selection();
                    } else {
                        state.copy_selection();
                    }
                    ui.ctx()
                        .copy_text(format!("{count} clip(s) copied from Tonique"));
                }
                Event::Paste(_) => {
                    state.paste();
                    reveal_selection(ui, state, viewport);
                }
                _ => {}
            }
        }
    }
}

/// Scroll to what an edit just selected (copies, pasted or moved clips).
fn reveal_selection(ui: &Ui, state: &mut ToniqueProjectState, viewport: Rect) {
    if let Some(range) = state.selection_range() {
        reveal(ui, state, viewport, range);
    }
}
