use egui::{Key, Ui};

use crate::{
    core::state::{PlaybackState, ToniqueProjectState},
    ui::view::timeline::UITimeline,
};

impl UITimeline {
    pub fn handle_key_press(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        // If other element focused do not check
        if ui.memory(|m| m.focused().is_some()) {
            return;
        }

        if ui.input(|i| i.focused && i.key_pressed(egui::Key::Space)) {
            if state.playback_state() == PlaybackState::Playing {
                state.pause();
            } else {
                state.play();
            }
        }

        if ui.input(|i| i.key_pressed(Key::D) && i.modifiers.ctrl) {
            // Duplicate clips
            if self.selected_clips.clip_ids.len() > 0 {
                state.duplicate_clips(
                    &self.selected_clips.clip_ids,
                    self.selected_clips.bounds.map(|b| (b.start_pos, b.end_pos)),
                );
                self.selected_clips.clip_ids.clear();
                if let Some(bounds) = &mut self.selected_clips.bounds {
                    let bound_size = bounds.end_pos - bounds.start_pos;
                    bounds.start_pos = bounds.end_pos;
                    bounds.end_pos += bound_size;
                }
            } else if let Some(selected) = state.selected_track() {
                state.duplicate_track(&selected.id);
            }
        } else if ui.memory(|m| m.focused().is_none())
            && ui.input(|i| {
                i.focused && i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)
            })
        {
            // Delete
            if self.selected_clips.clip_ids.len() > 0 {
                state.delete_clips(&self.selected_clips.clip_ids);
                self.selected_clips.reset();
            } else if let Some(selected) = state.selected_track() {
                state.delete_track(&selected.id);
            }
        } else if ui.input(|i| i.key_pressed(Key::K) && i.modifiers.ctrl) {
            // Cut clips
            for id in state.selected_tracks().clone() {
                state.cut_clip_at(&id, state.playback_position());
            }
        } else if ui.input(|i| i.key_pressed(Key::J) && i.modifiers.ctrl) {
            // Close bottom panel
            state.bottom_panel_open = !state.bottom_panel_open;
        } else if ui.input(|i| i.modifiers.ctrl && i.key_pressed(Key::Z)) {
            // Undo
            state.undo();
        } else if ui.input(|i| i.modifiers.ctrl && i.key_pressed(Key::Y)) {
            // Redo
            state.redo();
        }
    }
}
