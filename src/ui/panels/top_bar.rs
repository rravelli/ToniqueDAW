use egui::{Color32, FontId, Layout, Pos2, Rangef, Response, Sense, Stroke, Ui, Vec2};

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
const WAVE_LOW: Color32 = Color32::from_gray(60);
const WAVE_HIGH: Color32 = Color32::BLACK;

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
                if self.redo_ui(ui, state).clicked() {
                    state.redo();
                };
                if self.undo_ui(ui, state).clicked() {
                    state.undo();
                };
                self.usage_ui(ui, state);
                self.waveform_ui(ui, state);
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
            .tooltip(if playback_state == PlaybackState::Playing {
                "Pause"
            } else {
                "Play"
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
                .color(Color32::from_rgb(170, 10, 10))
                .tooltip("Record"),
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

    fn waveform_ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(35., ui.available_height()), Sense::hover());
        let painter = ui.painter_at(rect);

        // Background rectangle
        painter.rect_filled(rect, 1.0, PRIMARY_BUTTON_COLOR);

        // If we have waveform data
        if let Some(m) = state.metrics.tracks.get("master")
            && m.samples.len() >= 2
            && m.samples[0].len() > 3
        {
            let len = m.samples[0].len() as f32;
            let mut last_point = None;

            for (index, (l, r)) in m.samples[0].iter().zip(m.samples[1].iter()).enumerate() {
                let x = rect.left() + index as f32 * rect.width() / len;
                let y = rect.top() + rect.height() * (0.5 - (l + r) / 4.0);
                let pos = Pos2::new(x, y);

                // Gradient color based on amplitude intensity
                let amp = ((l.abs() + r.abs()) / 2.0).clamp(0.0, 1.0);
                let color = Color32::from_rgb(
                    (WAVE_LOW.r() as f32 * (1.0 - amp) + WAVE_HIGH.r() as f32 * amp) as u8,
                    (WAVE_LOW.g() as f32 * (1.0 - amp) + WAVE_HIGH.g() as f32 * amp) as u8,
                    (WAVE_LOW.b() as f32 * (1.0 - amp) + WAVE_HIGH.b() as f32 * amp) as u8,
                );

                // Draw connecting lines for smoother waveform
                if let Some(last) = last_point {
                    painter.line_segment([last, pos], Stroke::new(1.0, color));
                }
                last_point = Some(pos);
            }
        } else {
            // Draw center line if no waveform
            painter.line_segment(
                [rect.left_center(), rect.right_center()],
                Stroke::new(1.0, Color32::DARK_GRAY),
            );
        };
    }

    fn usage_ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) -> Response {
        ui.add(
            SquareButton::new(format!("{:.0}%", state.metrics.latency * 100.))
                .sized(25.)
                .font(FontId::new(10., egui::FontFamily::Proportional))
                .fill(PRIMARY_BUTTON_COLOR)
                .color(Color32::from_gray(30))
                .tooltip("CPU usage"),
        )
    }

    fn undo_ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) -> Response {
        ui.add_enabled(
            state.can_undo(),
            SquareButton::new(egui_phosphor::fill::ARROW_U_UP_LEFT)
                .sized(25.)
                .font(FontId::new(
                    15.,
                    egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                ))
                .fill(PRIMARY_BUTTON_COLOR)
                .color(Color32::from_gray(30))
                .tooltip("Undo"),
        )
    }

    fn redo_ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) -> Response {
        ui.add_enabled(
            state.can_redo(),
            SquareButton::new(egui_phosphor::fill::ARROW_U_UP_RIGHT)
                .sized(25.)
                .font(FontId::new(
                    15.,
                    egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                ))
                .fill(PRIMARY_BUTTON_COLOR)
                .color(Color32::from_gray(30))
                .tooltip("Redo"),
        )
    }
}
