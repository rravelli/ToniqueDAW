use egui::{Color32, FontId, Layout, Rangef, Response, Ui, Vec2};

use crate::{
    core::state::ToniqueProjectState,
    ui::{
        font::{PHOSPHOR_FILL, PHOSPHOR_REGULAR},
        widget::{input::NumberInput, square_button::SquareButton},
        workspace::PlaybackState,
    },
};

const PRIMARY_BUTTON_COLOR: Color32 = Color32::from_gray(150);
const SECONDARY_BUTTON_COLOR: Color32 = Color32::from_gray(100);
pub struct UITopBar {
    bpm_input: NumberInput,
}

impl UITopBar {
    pub fn new() -> Self {
        Self {
            bpm_input: NumberInput::new(Vec2::new(50., 25.))
                .fill(PRIMARY_BUTTON_COLOR)
                .text_color(Color32::from_gray(30))
                .with_range(Rangef::new(10., 1000.)),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
            self.metronome_ui(ui);
            if self.play_button_ui(ui, state.playback_state()).clicked() {
                if state.playback_state() == PlaybackState::Playing {
                    state.pause();
                } else {
                    state.play();
                }
            };
            self.record_button_ui(ui);
            self.bpm_input.value = state.bpm();
            self.bpm_input.ui(ui);
            if self.bpm_input.value != state.bpm() {
                state.set_bpm(self.bpm_input.value);
            }
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                self.redo_ui(ui);
                self.undo_ui(ui);
            });
        });
    }

    fn play_button_ui(&mut self, ui: &mut Ui, playback_state: PlaybackState) -> Response {
        ui.add(
            SquareButton::new(if playback_state == PlaybackState::Playing {
                egui_phosphor::fill::PAUSE
            } else {
                egui_phosphor::fill::PLAY
            })
            .sized(25.)
            .font(FontId::new(
                15.,
                egui::FontFamily::Name(PHOSPHOR_FILL.into()),
            ))
            .fill(if playback_state == PlaybackState::Playing {
                Color32::from_gray(200)
            } else {
                PRIMARY_BUTTON_COLOR
            })
            .color(Color32::from_gray(30)),
        )
    }

    fn record_button_ui(&mut self, ui: &mut Ui) {
        ui.add(
            SquareButton::new(egui_phosphor::fill::CIRCLE)
                .sized(25.)
                .font(FontId::new(
                    13.,
                    egui::FontFamily::Name(PHOSPHOR_FILL.into()),
                ))
                .fill(PRIMARY_BUTTON_COLOR)
                .color(Color32::from_rgb(170, 10, 10)),
        );
    }

    fn metronome_ui(&mut self, ui: &mut Ui) -> Response {
        ui.add(
            SquareButton::new(egui_phosphor::fill::METRONOME)
                .sized(25.)
                .font(FontId::new(
                    15.,
                    egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                ))
                .fill(SECONDARY_BUTTON_COLOR)
                .color(Color32::from_gray(30)),
        )
    }

    fn undo_ui(&mut self, ui: &mut Ui) -> Response {
        ui.add_enabled(
            false,
            SquareButton::new(egui_phosphor::fill::ARROW_U_UP_LEFT)
                .sized(25.)
                .font(FontId::new(
                    15.,
                    egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                ))
                .fill(PRIMARY_BUTTON_COLOR)
                .color(Color32::from_gray(30)),
        )
    }

    fn redo_ui(&mut self, ui: &mut Ui) -> Response {
        ui.add_enabled(
            false,
            SquareButton::new(egui_phosphor::fill::ARROW_U_UP_RIGHT)
                .sized(25.)
                .font(FontId::new(
                    15.,
                    egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                ))
                .fill(PRIMARY_BUTTON_COLOR)
                .color(Color32::from_gray(30)),
        )
    }
}
