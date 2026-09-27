use egui::{Event, Key, Rect, Ui};

use crate::{
    core::state::{PlaybackState, ToniqueProjectState},
    ui::view::timeline::{UITimeline, scroll::reveal},
};

impl UITimeline {
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

        if ui.input(|i| i.focused && i.key_pressed(egui::Key::Space)) {
            if state.playback_state() == PlaybackState::Playing {
                state.stop();
            } else {
                state.play();
            }
        }

        self.handle_clipboard(ui, state, viewport);
        // Left/Right: nudge the selected clips by a grid step, or move the
        // edit cursor when nothing is selected.
        let nudge = ui.input(|i| {
            if !i.modifiers.is_none() {
                0.
            } else if i.key_pressed(Key::ArrowLeft) {
                -1.
            } else if i.key_pressed(Key::ArrowRight) {
                1.
            } else {
                0.
            }
        });
        if nudge != 0. {
            let step = nudge * state.grid.step_beats();
            if state.selected_clips().is_empty() {
                let cursor = state.grid.snap_to_step(state.edit_cursor() + step);
                state.set_edit_cursor(cursor);
                reveal(ui, state, viewport, (cursor, cursor));
            } else {
                state.nudge_selection(step);
                reveal_selection(ui, state, viewport);
            }
        }

        // Up/Down: move the selected clips to the track above/below.
        let track_move = ui.input(|i| {
            if !i.modifiers.is_none() {
                0
            } else if i.key_pressed(Key::ArrowUp) {
                -1
            } else if i.key_pressed(Key::ArrowDown) {
                1
            } else {
                0
            }
        });
        if track_move != 0 {
            state.move_selection_tracks(track_move);
        }

        let duplicate_pressed = ui.input(|i| {
            i.events.iter().any(|event| {
                matches!(
                    event,
                    Event::Key {
                        key: Key::D,
                        pressed: true,
                        repeat: false,
                        modifiers,
                        ..
                    } if modifiers.ctrl
                )
            })
        });

        if duplicate_pressed {
            // Duplicate clips
            if !state.selected_clips().is_empty() {
                state.duplicate_selected_clips();
                reveal_selection(ui, state, viewport);
            } else if let Some(selected) = state.selected_track() {
                state.duplicate_track(&selected.id);
            }
        } else if ui
            .input(|i| i.focused && (i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)))
        {
            // Delete
            if !state.selected_clips().is_empty() {
                state.delete_selected_clips();
            } else if let Some(selected) = state.selected_track() {
                state.delete_track(&selected.id);
            }
        } else if ui.input(|i| i.key_pressed(Key::K) && i.modifiers.ctrl) {
            // Cut clips
            for id in state.selected_tracks().clone() {
                state.cut_clip_at(&id, state.edit_cursor());
            }
        } else if ui.input(|i| i.key_pressed(Key::L) && i.modifiers.ctrl) {
            // Loop the selection, or toggle looping
            if !state.loop_selection() {
                state.set_looping(!state.looping());
            }
        } else if ui.input(|i| i.key_pressed(Key::J) && i.modifiers.ctrl) {
            // Close bottom panel
            state.bottom_panel_open = !state.bottom_panel_open;
        } else if ui.input(|i| {
            i.modifiers.ctrl
                && (i.key_pressed(Key::Y) || i.modifiers.shift && i.key_pressed(Key::Z))
        }) {
            // Redo (Ctrl+Y or Ctrl+Shift+Z)
            state.redo();
        } else if ui.input(|i| i.modifiers.ctrl && i.key_pressed(Key::Z)) {
            // Undo
            state.undo();
        } else if ui.input(|i| i.modifiers.ctrl && i.key_pressed(Key::A)) {
            state.select_all_clips();
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
