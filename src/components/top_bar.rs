use egui::{Button, RichText, Ui, Vec2};

use crate::components::workspace::PlaybackState;

pub struct UITopBar {}

impl UITopBar {
    pub fn new() -> Self {
        Self {}
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        playback_state: PlaybackState,
        bpm: f32,
        update_bpm: &mut impl FnMut(f32) -> (),
    ) {
        ui.horizontal(|ui| {
            ui.add(
                Button::new(
                    RichText::new(egui_phosphor::fill::PLAY)
                        .family(egui::FontFamily::Name("phosphor_fill".into()))
                        .size(15.),
                )
                .corner_radius(1.0)
                .fill(if playback_state == PlaybackState::Playing {
                    ui.visuals().selection.bg_fill
                } else {
                    ui.visuals().faint_bg_color
                })
                .min_size(Vec2::new(25., 25.)),
            );

            ui.add(
                Button::new(
                    RichText::new(egui_phosphor::fill::STOP)
                        .size(15.)
                        .family(egui::FontFamily::Name("phosphor_fill".into())),
                )
                .corner_radius(1.0)
                .min_size(Vec2::new(25., 25.)),
            );

            ui.label(format!("{:.1}", bpm));
            if ui.button("+").clicked() {
                update_bpm(bpm + 1.);
            };
            if ui.button("-").clicked() {
                update_bpm(bpm - 1.)
            };
        });
    }
}
