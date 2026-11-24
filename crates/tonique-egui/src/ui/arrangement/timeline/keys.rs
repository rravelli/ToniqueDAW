use egui::{Event, Rect, Ui};

use crate::ui::{
    arrangement::timeline::{Timeline, scroll::reveal},
    commands::Commands,
    widget::arrow_keys::holds_arrows,
};
use tonique_core::{config::keymap::Action, project::ActionOutcome, project::ProjectState};

impl Timeline {
    /// Run the queued actions acting on the selection or edit cursor, and
    /// clipboard events. The app runs the other actions.
    pub fn run_actions(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        commands: &mut Commands,
        viewport: Rect,
    ) {
        for action in commands.take(Action::is_timeline) {
            self.run_action(action, ui, state, viewport);
        }
        // Not while typing into a widget.
        if ui.memory(|m| m.focused().is_none()) || holds_arrows(ui.ctx()) {
            self.handle_clipboard(ui, state, viewport);
        }
    }

    fn run_action(
        &mut self,
        action: Action,
        ui: &mut Ui,
        state: &mut ProjectState,
        viewport: Rect,
    ) {
        if let ActionOutcome::Reveal(start, end) = state.run_action(action) {
            reveal(ui, state, viewport, (start, end));
        }
    }

    /// Ctrl+C/X/V. eframe reports them as clipboard events rather than
    /// keys, and only reports a paste when the system clipboard holds text,
    /// so copying also puts a line of text there.
    fn handle_clipboard(&mut self, ui: &mut Ui, state: &mut ProjectState, viewport: Rect) {
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
                    if let ActionOutcome::Reveal(start, end) = state.paste_and_reveal() {
                        reveal(ui, state, viewport, (start, end));
                    }
                }
                _ => {}
            }
        }
    }
}
