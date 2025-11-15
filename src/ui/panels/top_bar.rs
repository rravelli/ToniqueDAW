use cpal::{
    available_hosts, default_host, host_from_id,
    traits::{DeviceTrait, HostTrait},
};
use egui::{
    Color32, Context, FontFamily, FontId, Frame, Layout, Margin, Pos2, Rangef, Response, Sense,
    Stroke, Ui, Vec2, Widget, containers::menu::MenuButton,
};
use egui_phosphor::{
    fill::{
        ARROW_COUNTER_CLOCKWISE, ARROWS_COUNTER_CLOCKWISE, EXPORT, FLOPPY_DISK, SIDEBAR_SIMPLE,
    },
    regular::RECORD,
};

use crate::{
    core::state::{PlaybackState, ToniqueProjectState},
    ui::{
        font::{PHOSPHOR_FILL, PHOSPHOR_REGULAR},
        theme::PRIMARY_COLOR,
        widget::{
            context_menu::{ContextMenuButton, ContextMenuSeparator},
            input::NumberInput,
            square_button::SquareButton,
        },
    },
};
const BUTTON_SIZE: f32 = 22.;
const PRIMARY_BUTTON_COLOR: Color32 = Color32::from_gray(150);
const WAVE_LOW: Color32 = Color32::from_gray(60);
const WAVE_HIGH: Color32 = Color32::BLACK;

pub struct UITopBar {
    bpm_input: NumberInput,
}

impl UITopBar {
    pub fn new() -> Self {
        Self {
            bpm_input: NumberInput::new(Vec2::new(48., BUTTON_SIZE))
                .fill(PRIMARY_BUTTON_COLOR)
                .text_color(Color32::from_gray(30))
                .with_range(Rangef::new(10., 1000.)),
        }
    }

    pub fn show(&mut self, ctx: &Context, state: &mut ToniqueProjectState) {
        egui::TopBottomPanel::top("top-bar")
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(Color32::from_gray(40))
                    .inner_margin(Margin::same(4)),
            )
            .show(ctx, |ui| {
                self.ui(ui, state);
            });
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
            self.sidebar_ui(ui, state);

            MenuButton::new("File").ui(ui, |ui| {
                ContextMenuButton::new("", "New").submenu(ui, |ui| {});
                ContextMenuButton::new(FLOPPY_DISK, "Save").ui(ui);
                ContextMenuSeparator::new().ui(ui);
                if ContextMenuButton::new(EXPORT, "Export").ui(ui).clicked() {
                    state.show_export = true;
                }
            });

            self.metronome_ui(ui, state);
            self.bpm_input.value = state.bpm();
            self.bpm_input.ui(ui);
            if self.bpm_input.value != state.bpm() {
                state.set_bpm(self.bpm_input.value);
            }
            if self.play_button_ui(ui, state.playback_state()).clicked() {
                if state.playback_state() == PlaybackState::Playing {
                    state.pause();
                } else {
                    state.play();
                }
            };
            self.record_button_ui(ui);
            self.loop_button_ui(ui, state);

            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if self.redo_ui(ui, state).clicked() {
                    state.redo();
                };
                if self.undo_ui(ui, state).clicked() {
                    state.undo();
                };
                self.output_ui(ui, state);
                self.usage_ui(ui, state);
                self.fps_ui(ui);
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
            .square(BUTTON_SIZE)
            .font(FontId::new(
                12.,
                egui::FontFamily::Name(PHOSPHOR_FILL.into()),
            ))
            .fill(if playback_state == PlaybackState::Playing {
                PRIMARY_COLOR
            } else {
                PRIMARY_BUTTON_COLOR
            })
            .color(Color32::from_gray(30)),
        )
    }

    fn record_button_ui(&mut self, ui: &mut Ui) {
        ui.add(
            SquareButton::new(RECORD)
                .square(BUTTON_SIZE)
                .font(FontId::new(
                    14.,
                    egui::FontFamily::Name(PHOSPHOR_FILL.into()),
                ))
                .fill(PRIMARY_BUTTON_COLOR)
                .color(Color32::from_gray(30))
                .tooltip("Record"),
        );
    }

    fn loop_button_ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        if ui
            .add(
                SquareButton::new(ARROWS_COUNTER_CLOCKWISE)
                    .square(BUTTON_SIZE)
                    .font(FontId::new(
                        14.,
                        egui::FontFamily::Name(PHOSPHOR_FILL.into()),
                    ))
                    .fill(if state.loop_state.enabled {
                        PRIMARY_COLOR
                    } else {
                        PRIMARY_BUTTON_COLOR
                    })
                    .color(Color32::from_gray(30))
                    .tooltip("Loop"),
            )
            .clicked()
        {
            state.loop_state.enabled = !state.loop_state.enabled
        }
    }

    fn sidebar_ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) -> Response {
        let res = ui.add(
            SquareButton::ghost(SIDEBAR_SIMPLE)
                .square(BUTTON_SIZE)
                .font(FontId::new(
                    15.,
                    if state.left_panel_open {
                        egui::FontFamily::Name(PHOSPHOR_FILL.into())
                    } else {
                        egui::FontFamily::Name(PHOSPHOR_REGULAR.into())
                    },
                ))
                .color(if state.left_panel_open {
                    PRIMARY_COLOR
                } else {
                    Color32::from_gray(180)
                }),
        );

        if res.clicked() {
            state.left_panel_open = !state.left_panel_open;
        };

        res
    }

    fn metronome_ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) -> Response {
        let click = state.metronome()
            && matches!(state.playback_state(), PlaybackState::Playing)
            && state.playback_position() % 1.0 < 0.5;
        let res = ui.add(
            SquareButton::new(egui_phosphor::fill::METRONOME)
                .square(BUTTON_SIZE)
                .font(FontId::new(
                    15.,
                    if state.metronome() {
                        FontFamily::Name(PHOSPHOR_FILL.into())
                    } else {
                        egui::FontFamily::Name(PHOSPHOR_REGULAR.into())
                    },
                ))
                .fill(if state.metronome() {
                    PRIMARY_COLOR
                } else {
                    PRIMARY_BUTTON_COLOR
                })
                .color(if click {
                    Color32::from_black_alpha(70)
                } else {
                    Color32::from_gray(30)
                }),
        );
        if res.clicked() {
            state.toggle_metronome();
        }

        res
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
            SquareButton::new(format!(
                "{:.0}%",
                (state.metrics.processing_ratio * 100.).round()
            ))
            .square(BUTTON_SIZE)
            .font(FontId::new(10., egui::FontFamily::Proportional))
            .fill(PRIMARY_BUTTON_COLOR)
            .color(Color32::from_gray(30))
            .tooltip("CPU usage"),
        )
    }

    fn fps_ui(&mut self, ui: &mut Ui) -> Response {
        ui.add(
            SquareButton::new(format!(
                "{:.0}",
                1.0 / ui.ctx().input(|i| i.stable_dt).max(1e-5)
            ))
            .square(BUTTON_SIZE)
            .font(FontId::new(10., egui::FontFamily::Proportional))
            .fill(PRIMARY_BUTTON_COLOR)
            .color(Color32::from_gray(30))
            .tooltip("FPS"),
        )
    }

    fn output_ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) -> Response {
        let res = ui.add(
            SquareButton::new(state.output_device().unwrap_or("OFF".into()))
                .square(BUTTON_SIZE)
                .font(FontId::new(10., egui::FontFamily::Proportional))
                .fill(PRIMARY_BUTTON_COLOR)
                .color(Color32::from_gray(30))
                .tooltip("Output device"),
        );

        if res.clicked() {
            let host = default_host();

            println!("{:?}", available_hosts());
            println!(
                "{:?}",
                host.output_devices()
                    .unwrap()
                    .map(|d| d.name())
                    .collect::<Vec<_>>()
            );
            println!(
                "{:?}",
                host.input_devices()
                    .unwrap()
                    .map(|d| d.name())
                    .collect::<Vec<_>>()
            );
        }

        res
    }

    fn undo_ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) -> Response {
        ui.add_enabled(
            state.can_undo(),
            SquareButton::ghost(egui_phosphor::fill::ARROW_U_UP_LEFT)
                .square(BUTTON_SIZE)
                .font(FontId::new(
                    15.,
                    egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                ))
                .tooltip("Undo"),
        )
    }

    fn redo_ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) -> Response {
        ui.add_enabled(
            state.can_redo(),
            SquareButton::ghost(egui_phosphor::fill::ARROW_U_UP_RIGHT)
                .square(BUTTON_SIZE)
                .font(FontId::new(
                    15.,
                    egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                ))
                .tooltip("Redo"),
        )
    }
}
