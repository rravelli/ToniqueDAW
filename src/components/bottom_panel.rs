use crate::{
    components::{
        effect::{self, UIEffect, UIEffectContent},
        effects::{create_effect_from_id, equalizer::EqualizerEffect},
        left_panel::{DragPayload, DraggedItem},
        track::UITrack,
    },
    message::GuiToPlayerMsg,
    metrics::AudioMetrics,
};
use egui::{
    Color32, Frame, Key, Layout, Margin, Pos2, RichText, ScrollArea, Separator, Stroke, Ui, Vec2,
};
use fundsp::{
    hacker::lowpass,
    hacker32::{lowpass_hz, pass, shared, var},
};
use rtrb::Producer;

pub const BOTTOM_BAR_HEIGHT: f32 = 20.;

pub struct UIBottomPanel {
    pub open: bool,
    selected: Vec<usize>,
    offset: f32,
    insert_index: Option<usize>,
}

impl UIBottomPanel {
    pub fn new() -> Self {
        Self {
            selected: vec![],
            open: false,
            offset: 0.,
            insert_index: None,
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        track: &mut UITrack,
        metrics: &mut AudioMetrics,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) {
        let mut insert_index = None;
        let mut drag_payload = None;

        // Payload hovered
        if let Some(payload) = ui.response().dnd_hover_payload::<DragPayload>()
            && let DragPayload::Effect(_) = *payload
        {
            insert_index = Some(track.effects_mut().len());
            drag_payload = Some(payload);
        }

        // Payload released
        if let Some(payload) = ui.response().dnd_release_payload::<DragPayload>()
            && let Some(index) = self.insert_index
            && let DragPayload::Effect(effect_id) = *payload
        {
            track.add_effect(effect_id, 0, tx);
        }

        self.top_bar(ui, track);

        let effects_len = track.effects_mut().len();
        let inner = ScrollArea::horizontal().show(ui, |ui| {
            ui.allocate_ui_with_layout(
                ui.available_size(),
                Layout::left_to_right(egui::Align::Min),
                |ui| {
                    let effects = track.effects_mut();
                    for (i, effect) in effects.iter_mut().enumerate() {
                        // Add space
                        if let Some(index) = self.insert_index
                            && index == i
                        {
                            ui.add(Separator::default().vertical().spacing(8.));
                        } else {
                            ui.add_space(8.);
                        }

                        let response = effect.ui(ui, metrics, self.selected.contains(&i), tx);

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
                },
            );
        });

        if ui.input(|i| i.key_pressed(Key::Delete)) && self.selected.len() > 0 {
            track.remove_effects(&self.selected, tx);
        }

        self.insert_index = insert_index;
        self.offset = inner.state.offset.x;
    }

    fn top_bar(&mut self, ui: &mut Ui, track: &UITrack) {
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
                        RichText::new(track.name.clone())
                            .size(10.)
                            .color(Color32::from_gray(20)),
                    );
                });
            });
    }
}
