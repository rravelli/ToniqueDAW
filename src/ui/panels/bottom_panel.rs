use crate::{
    core::{metrics::AudioMetrics, state::ToniqueProjectState, track::TrackReferenceCore},
    ui::panels::left_panel::DragPayload,
    utils::parse_name,
};
use egui::{
    Color32, Context, Frame, Key, Layout, Margin, Rangef, RichText, ScrollArea, Separator, Stroke,
    Ui,
};

pub const BOTTOM_BAR_HEIGHT: f32 = 20.;

pub struct UIBottomPanel {
    selected: Vec<usize>,
    offset: f32,
    insert_index: Option<usize>,
}

impl UIBottomPanel {
    pub fn new() -> Self {
        Self {
            selected: vec![],
            offset: 0.,
            insert_index: None,
        }
    }

    pub fn show(&mut self, ctx: &Context, state: &mut ToniqueProjectState) {
        egui::TopBottomPanel::bottom("bottom-panel")
            .height_range(Rangef::new(50. + BOTTOM_BAR_HEIGHT, 400.))
            .resizable(true)
            .frame(Frame::new().inner_margin(Margin::ZERO))
            .show_animated(ctx, state.bottom_panel_open, |ui| {
                ui.set_height(ui.available_height());

                if let Some(selected) = state.selected_track() {
                    self.ui(ui, selected, state);
                }
            });
    }

    pub fn ui(&mut self, ui: &mut Ui, track: TrackReferenceCore, state: &mut ToniqueProjectState) {
        // TODO Not using unwrap
        let mut metrics = state
            .metrics
            .tracks
            .get(&track.id)
            .unwrap_or(&AudioMetrics::new())
            .clone();

        let mut insert_index = None;
        let mut drag_payload = None;

        // Payload hovered
        if let Some(payload) = ui.response().dnd_hover_payload::<DragPayload>()
            && let DragPayload::Effect(_) = *payload
        {
            insert_index = Some(state.effects_mut(&track.id).map_or(0, |e| e.len()));
            drag_payload = Some(payload);
        }

        // Payload released
        if let Some(payload) = ui.response().dnd_release_payload::<DragPayload>()
            && let Some(index) = self.insert_index
            && let DragPayload::Effect(effect_id) = *payload
        {
            state.add_effect(&track.id, effect_id, index);
        }

        self.top_bar(ui, &track);

        let effects_len = state.effects_mut(&track.id).map_or(0, |t| t.len());
        let inner = ScrollArea::horizontal().show(ui, |ui| {
            ui.allocate_ui_with_layout(
                ui.available_size(),
                Layout::left_to_right(egui::Align::Min),
                |ui| {
                    if let Some(effects) = state.effects_mut(&track.id).take() {
                        for (i, effect) in effects.iter_mut().enumerate() {
                            // Add space
                            if let Some(index) = self.insert_index
                                && index == i
                            {
                                ui.add(Separator::default().vertical().spacing(8.));
                            } else {
                                ui.add_space(8.);
                            }

                            let response = effect.ui(ui, &mut metrics, self.selected.contains(&i));

                            // Select effect
                            if response.inner.clicked() {
                                if self.selected.contains(&i) {
                                    self.selected = vec![];
                                } else {
                                    self.selected = vec![i];
                                }
                            }

                            // Update insertion index
                            if drag_payload.is_some()
                                && let Some(mouse_pos) = ui.input(|i| i.pointer.interact_pos())
                                && response.response.rect.contains(mouse_pos)
                            {
                                insert_index = Some(i);
                            }

                            response
                                .inner
                                .on_hover_and_drag_cursor(egui::CursorIcon::Grab);
                        }
                        if let Some(index) = self.insert_index
                            && index == effects_len
                        {
                            ui.add(Separator::default().vertical().spacing(8.));
                        }
                    }
                },
            );
        });

        if ui.input(|i| i.key_pressed(Key::Delete)) && self.selected.len() > 0 {
            state.remove_effects(&track.id, &self.selected);
        }

        self.insert_index = insert_index;
        self.offset = inner.state.offset.x;
    }

    fn top_bar(&mut self, ui: &mut Ui, track: &TrackReferenceCore) {
        Frame::new()
            .fill(track.color)
            .stroke(Stroke::new(1.0, Color32::DARK_GRAY))
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
                        RichText::new(parse_name(&track.name, track.index))
                            .size(10.)
                            .color(Color32::from_gray(20)),
                    );
                });
            });
    }
}
