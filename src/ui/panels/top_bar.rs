use egui::{
    FontFamily, FontId, Frame, Layout, Margin, Pos2, Rangef, Response, Sense, Stroke, Ui, Vec2,
};
use egui_phosphor::{
    fill::SIDEBAR_SIMPLE,
    regular::{GRAPH, RECORD},
};

use crate::{
    config::keymap::Action,
    core::state::{MASTER_TRACK_ID, PlaybackState, ToniqueProjectState},
    ui::{
        font::{PHOSPHOR_FILL, PHOSPHOR_REGULAR},
        theme::{ThemeExt, with_alpha},
        widget::{input::NumberInput, square_button::SquareButton},
        workspace::{MainView, Workspace},
    },
};
const BUTTON_SIZE: f32 = 22.;

pub struct TopBar {
    bpm_input: NumberInput,
}

impl TopBar {
    pub fn new() -> Self {
        Self {
            bpm_input: NumberInput::new(Vec2::new(50., BUTTON_SIZE))
                .with_range(Rangef::new(10., 1000.)),
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        state: &mut ToniqueProjectState,
        workspace: &mut Workspace,
    ) {
        egui::Panel::top("top-bar")
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(ui.app_theme().bg_panel)
                    .inner_margin(Margin::same(4)),
            )
            .show(ui, |ui| {
                self.ui(ui, state, workspace);
            });
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState, workspace: &mut Workspace) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
            self.sidebar_ui(ui, state, workspace);
            self.graph_view_ui(ui, state, workspace);
            self.metronome_ui(ui, state);
            if self.play_button_ui(ui, state).clicked() {
                if state.playback_state() == PlaybackState::Playing {
                    state.stop();
                } else {
                    state.play();
                }
            };
            self.record_button_ui(ui);
            self.loop_ui(ui, state);
            self.follow_ui(ui, state);
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
                self.fps_ui(ui);
                self.waveform_ui(ui, state);
            });
        });
    }

    fn play_button_ui(&mut self, ui: &mut Ui, state: &ToniqueProjectState) -> Response {
        let playback_state = state.playback_state();
        let tooltip = tooltip(
            ui,
            state,
            if playback_state == PlaybackState::Playing {
                "Stop"
            } else {
                "Play"
            },
            Action::PlayStop,
        );
        ui.add(
            SquareButton::new(if playback_state == PlaybackState::Playing {
                egui_phosphor::fill::STOP
            } else {
                egui_phosphor::fill::PLAY
            })
            .tooltip(tooltip)
            .square(BUTTON_SIZE)
            .font(FontId::new(
                12.,
                egui::FontFamily::Name(PHOSPHOR_FILL.into()),
            ))
            .selected(playback_state == PlaybackState::Playing),
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
                .color(ui.app_theme().record)
                .tooltip("Record"),
        );
    }

    fn sidebar_ui(
        &mut self,
        ui: &mut Ui,
        state: &ToniqueProjectState,
        workspace: &mut Workspace,
    ) -> Response {
        let res = ui.add(
            SquareButton::ghost(SIDEBAR_SIMPLE)
                .square(BUTTON_SIZE)
                .font(FontId::new(
                    15.,
                    if workspace.left_panel_open {
                        egui::FontFamily::Name(PHOSPHOR_FILL.into())
                    } else {
                        egui::FontFamily::Name(PHOSPHOR_REGULAR.into())
                    },
                ))
                .selected(workspace.left_panel_open)
                .tooltip(tooltip(ui, state, "Browser", Action::ToggleBrowser)),
        );

        if res.clicked() {
            workspace.left_panel_open = !workspace.left_panel_open;
        };

        res
    }

    fn graph_view_ui(
        &mut self,
        ui: &mut Ui,
        state: &ToniqueProjectState,
        workspace: &mut Workspace,
    ) -> Response {
        let active = workspace.main_view == MainView::Graph;
        let res = ui.add(
            SquareButton::ghost(GRAPH)
                .square(BUTTON_SIZE)
                .font(FontId::new(
                    15.,
                    egui::FontFamily::Name(
                        if active {
                            PHOSPHOR_FILL
                        } else {
                            PHOSPHOR_REGULAR
                        }
                        .into(),
                    ),
                ))
                .selected(active)
                .tooltip(tooltip(ui, state, "Audio graph", Action::ToggleGraphView)),
        );

        if res.clicked() {
            workspace.toggle_graph();
        };

        res
    }

    fn follow_ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        let follow = state.follow_playhead();
        let res = ui.add(
            SquareButton::new(egui_phosphor::fill::CARET_LINE_RIGHT)
                .square(BUTTON_SIZE)
                .font(FontId::new(
                    14.,
                    FontFamily::Name(
                        if follow {
                            PHOSPHOR_FILL
                        } else {
                            PHOSPHOR_REGULAR
                        }
                        .into(),
                    ),
                ))
                .selected(follow)
                .tooltip(tooltip(
                    ui,
                    state,
                    "Follow playhead",
                    Action::ToggleFollowPlayhead,
                )),
        );
        if res.clicked() {
            state.set_follow_playhead(!follow);
        }
    }

    fn loop_ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        let looping = state.looping();
        let res = ui.add(
            SquareButton::new(egui_phosphor::fill::REPEAT)
                .square(BUTTON_SIZE)
                .font(FontId::new(
                    14.,
                    FontFamily::Name(
                        if looping {
                            PHOSPHOR_FILL
                        } else {
                            PHOSPHOR_REGULAR
                        }
                        .into(),
                    ),
                ))
                .selected(looping)
                .tooltip(tooltip(ui, state, "Loop", Action::Loop)),
        );
        if res.clicked() {
            state.set_looping(!looping);
        }
    }

    fn metronome_ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) -> Response {
        let theme = ui.app_theme();
        let click = state.metronome()
            && matches!(state.playback_state(), PlaybackState::Playing)
            && state.playhead() % 1.0 < 0.5;
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
                .selected(state.metronome())
                // Blink on the beat.
                .color(match (state.metronome(), click) {
                    (false, _) => theme.text,
                    (true, false) => theme.text_on_accent,
                    (true, true) => with_alpha(theme.text_on_accent, 110),
                })
                .tooltip(tooltip(ui, state, "Metronome", Action::ToggleMetronome)),
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
        let theme = ui.app_theme();
        painter.rect_filled(rect, 1.0, theme.bg_deep);

        // If we have waveform data
        if let Some(m) = state.metrics.tracks.get(&MASTER_TRACK_ID)
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
                let color = theme.text_disabled.lerp_to_gamma(theme.accent, amp);

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
                Stroke::new(1.0, theme.separator),
            );
        };
    }

    fn usage_ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) -> Response {
        ui.add(
            SquareButton::new(format!("{:.0}%", (state.metrics.latency * 100.).round()))
                .square(BUTTON_SIZE)
                .font(FontId::new(10., egui::FontFamily::Proportional))
                .color(ui.app_theme().text_muted)
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
            .color(ui.app_theme().text_muted)
            .tooltip("FPS"),
        )
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
                .tooltip(tooltip(ui, state, "Undo", Action::Undo)),
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
                .tooltip(tooltip(ui, state, "Redo", Action::Redo)),
        )
    }
}

/// `text` with the shortcut of `action`, e.g. `Loop (Ctrl+L)`.
fn tooltip(ui: &Ui, state: &ToniqueProjectState, text: &str, action: Action) -> String {
    state
        .settings()
        .keymap
        .with_shortcut(ui.ctx(), text, action)
}
