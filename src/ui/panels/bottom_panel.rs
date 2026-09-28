use crate::{
    config::keymap::Action,
    core::{metrics::AudioMetrics, state::ProjectState, track::TrackRow},
    ui::{
        commands::Commands, dnd::DragPayload, effects::EffectRack, theme::ThemeExt,
        workspace::Workspace,
    },
    utils::display_name,
};
use egui::{Frame, Layout, Margin, Rangef, RichText, ScrollArea, Separator, Stroke, Ui};
use tonique_engine::edit::TrackId;

pub const HEADER_HEIGHT: f32 = 20.;

pub struct BottomPanel {
    rack: EffectRack,
    /// Selected effects, by index, of `track`'s. Delete removes them.
    selected: Vec<usize>,
    track: Option<TrackId>,
    offset: f32,
    insert_index: Option<usize>,
}

impl BottomPanel {
    pub fn new() -> Self {
        Self {
            rack: EffectRack::default(),
            selected: vec![],
            track: None,
            offset: 0.,
            insert_index: None,
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        workspace: &mut Workspace,
        commands: &mut Commands,
    ) {
        let mut open = workspace.bottom_panel_open;
        egui::Panel::bottom("bottom-panel")
            .size_range(Rangef::new(50. + HEADER_HEIGHT, 400.))
            .resizable(true)
            .frame(Frame::new().inner_margin(Margin::ZERO))
            .show_collapsible(ui, &mut open, |ui| {
                ui.set_height(ui.available_height());

                if let Some(selected) = state.selected_track() {
                    self.ui(ui, selected, state, commands);
                }
            });
        workspace.bottom_panel_open = open;
        // The selection only holds while its effects are shown.
        if !open {
            self.selected.clear();
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        track: TrackRow,
        state: &mut ProjectState,
        commands: &mut Commands,
    ) {
        if self.track != Some(track.id) {
            self.selected.clear();
            self.track = Some(track.id);
        }
        // Clicking elsewhere leaves the effects.
        let panel = ui.max_rect();
        if ui.input(|i| {
            i.pointer.primary_pressed()
                && i.pointer.interact_pos().is_some_and(|p| !panel.contains(p))
        }) {
            self.selected.clear();
        }
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

        // Delete acts on the selected effects rather than the timeline's
        // selection.
        if !self.selected.is_empty() && !commands.take(|a| a == Action::Delete).is_empty() {
            state.remove_effects(&track.id, &self.selected);
            self.selected.clear();
        }

        self.insert_index = insert_index;
        self.offset = inner.state.offset.x;
    }

    fn header(&mut self, ui: &mut Ui, track: &TrackRow) {
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
        let mut state = ProjectState::new(engine);
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
        let mut workspace = Workspace {
            bottom_panel_open: true,
            ..Default::default()
        };

        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let mut panel = BottomPanel::new();
        let mut shapes = 0;
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200., 800.))),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                panel.show(ui, &mut state, &mut workspace, &mut Commands::default())
            });
            output.textures_delta.clear();
            shapes = output.shapes.len();
        }
        assert!(shapes > 0);
        assert_eq!(panel.rack.len(), 2, "an editor per effect");
        assert_eq!(cutoff.get(), 440.);
    }

    /// Delete removes the selected effects, and only then: otherwise it's
    /// left for the timeline.
    #[test]
    fn delete_goes_to_selected_effects() {
        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        let track = state.add_track();
        state.add_effect(&track, EffectKind::Filter, 0);
        state.add_effect(&track, EffectKind::Filter, 1);
        state.select_track(&track);
        let mut workspace = Workspace {
            bottom_panel_open: true,
            ..Default::default()
        };
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let mut panel = BottomPanel::new();
        let mut frame = |panel: &mut BottomPanel, state: &mut ProjectState, delete: bool| {
            let mut commands = Commands::default();
            if delete {
                commands.push(Action::Delete);
            }
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200., 800.))),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                panel.show(ui, state, &mut workspace, &mut commands)
            });
            output.textures_delta.clear();
            commands.take_all()
        };
        frame(&mut panel, &mut state, false);

        assert_eq!(frame(&mut panel, &mut state, true), [Action::Delete]);
        assert_eq!(state.effects(&track).len(), 2);

        panel.selected = vec![0];
        assert!(frame(&mut panel, &mut state, true).is_empty());
        assert_eq!(state.effects(&track).len(), 1);
        assert!(panel.selected.is_empty());
    }
}
