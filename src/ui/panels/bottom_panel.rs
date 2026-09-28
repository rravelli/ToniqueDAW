use crate::{
    core::{metrics::AudioMetrics, state::ToniqueProjectState, track::TrackReferenceCore},
    ui::{effects::EffectRack, panels::left_panel::DragPayload, theme::ThemeExt},
    utils::display_name,
};
use egui::{Frame, Key, Layout, Margin, Rangef, RichText, ScrollArea, Separator, Stroke, Ui};

pub const HEADER_HEIGHT: f32 = 20.;

pub struct BottomPanel {
    rack: EffectRack,
    selected: Vec<usize>,
    offset: f32,
    insert_index: Option<usize>,
}

impl BottomPanel {
    pub fn new() -> Self {
        Self {
            rack: EffectRack::default(),
            selected: vec![],
            offset: 0.,
            insert_index: None,
        }
    }

    pub fn show(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        let mut open = state.bottom_panel_open;
        egui::Panel::bottom("bottom-panel")
            .size_range(Rangef::new(50. + HEADER_HEIGHT, 400.))
            .resizable(true)
            .frame(Frame::new().inner_margin(Margin::ZERO))
            .show_collapsible(ui, &mut open, |ui| {
                ui.set_height(ui.available_height());

                if let Some(selected) = state.selected_track() {
                    self.ui(ui, selected, state);
                }
            });
        state.bottom_panel_open = open;
    }

    pub fn ui(&mut self, ui: &mut Ui, track: TrackReferenceCore, state: &mut ToniqueProjectState) {
        let mut metrics = state
            .metrics
            .tracks
            .get(&track.id)
            .cloned()
            .unwrap_or_else(AudioMetrics::new);
        let mut insert_index = None;
        let mut drag_payload = None;

        // Payload hovered
        if let Some(payload) = ui.response().dnd_hover_payload::<DragPayload>()
            && let DragPayload::Effect(_) = *payload
        {
            insert_index = Some(state.effects(&track.id).len());
            drag_payload = Some(payload);
        }

        // Payload released
        if let Some(payload) = ui.response().dnd_release_payload::<DragPayload>()
            && let Some(index) = self.insert_index
            && let DragPayload::Effect(effect_id) = *payload
        {
            state.add_effect(&track.id, effect_id, index);
        }

        self.header(ui, &track);

        let effects = state.effects(&track.id);
        let mut toggled = None;
        let inner = ScrollArea::horizontal().show(ui, |ui| {
            ui.allocate_ui_with_layout(
                ui.available_size(),
                Layout::left_to_right(egui::Align::Min),
                |ui| {
                    for (i, effect) in effects.iter().enumerate() {
                        // Add space
                        if let Some(index) = self.insert_index
                            && index == i
                        {
                            ui.add(Separator::default().vertical().spacing(8.));
                        } else {
                            ui.add_space(8.);
                        }

                        let selected = self.selected.contains(&i);
                        let response = self.rack.effect_ui(ui, effect, &mut metrics, selected);
                        if response.toggled {
                            toggled = Some(effect);
                        }

                        // Select effect
                        if response.header.clicked() {
                            if selected {
                                self.selected = vec![];
                            } else {
                                self.selected = vec![i];
                            }
                        }

                        // Update insertion index
                        if drag_payload.is_some()
                            && let Some(mouse_pos) = ui.input(|i| i.pointer.interact_pos())
                            && response.rect.contains(mouse_pos)
                        {
                            insert_index = Some(i);
                        }

                        response
                            .header
                            .on_hover_and_drag_cursor(egui::CursorIcon::Grab);
                    }
                    if let Some(index) = self.insert_index
                        && index == effects.len()
                    {
                        ui.add(Separator::default().vertical().spacing(8.));
                    }
                },
            );
        });
        if let Some(effect) = toggled {
            state.set_effect_enabled(&track.id, effect.plugin.id, !effect.enabled());
        }

        // Not while a widget has focus, e.g. the shortcut recorder.
        if ui.input(|i| i.key_pressed(Key::Delete))
            && ui.memory(|m| m.focused().is_none())
            && self.selected.len() > 0
        {
            state.remove_effects(&track.id, &self.selected);
        }

        self.insert_index = insert_index;
        self.offset = inner.state.offset.x;
    }

    fn header(&mut self, ui: &mut Ui, track: &TrackReferenceCore) {
        let theme = ui.app_theme();
        Frame::new()
            .fill(track.color)
            .stroke(Stroke::new(1.0, theme.border))
            .inner_margin(Margin {
                bottom: 1,
                top: 1,
                left: 5,
                right: 5,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(display_name(&track.name, track.index))
                            .size(10.)
                            .color(theme.text_on(track.color)),
                    );
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::effect::EffectKind;
    use egui::{Rect, vec2};
    use tonique_engine::engine::{Engine, EngineConfig};

    /// The selected track's effects draw, headless, and their editors don't
    /// touch the parameters by just being shown.
    #[test]
    fn draws_the_selected_tracks_effects_headless() {
        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ToniqueProjectState::new(engine);
        let track = state.add_track();
        state.add_effect(&track, EffectKind::Filter, 0);
        state.add_effect(&track, EffectKind::Filter, 1);
        let cutoff = state.effects(&track)[1]
            .plugin
            .param("cutoff")
            .unwrap()
            .clone();
        cutoff.set(440.);
        state.select_track(&track);
        state.bottom_panel_open = true;

        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let mut panel = BottomPanel::new();
        let mut shapes = 0;
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200., 800.))),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| panel.show(ui, &mut state));
            output.textures_delta.clear();
            shapes = output.shapes.len();
        }
        assert!(shapes > 0);
        assert_eq!(panel.rack.len(), 2, "an editor per effect");
        assert_eq!(cutoff.get(), 440.);
    }
}
