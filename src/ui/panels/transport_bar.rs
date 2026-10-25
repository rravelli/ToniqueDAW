use crate::ui::theme::font_size;
use egui::{
    Align, Align2, FontFamily, FontId, Frame, Layout, Margin, Rangef, Rect, Response, RichText,
    Sense, Ui, UiBuilder, Vec2, pos2, vec2,
};
use egui_phosphor::{
    fill::SIDEBAR_SIMPLE,
    regular::{CPU, GRAPH, RECORD, SPEAKER_SLASH},
};
use tonique_engine::time::TimeSignature;

use crate::{
    config::keymap::Action,
    core::state::{PlaybackState, ProjectState},
    ui::{
        RECORDING,
        commands::Commands,
        font::{PHOSPHOR_FILL, PHOSPHOR_REGULAR},
        theme::{ThemeExt, with_alpha},
        widget::{flat_button::FlatButton, input::NumberInput},
        workspace::{MainView, Workspace},
    },
    utils::{format_bars, format_time},
};
const BUTTON_SIZE: f32 = 22.;
/// Between the buttons of a group.
const BUTTON_GAP: f32 = 2.;
/// Between groups.
const GROUP_GAP: f32 = 8.;
/// The playhead's position: bars and time.
const POSITION_WIDTH: f32 = 140.;
/// The position in bars alone, when the bar is too narrow for the time.
const POSITION_BARS_WIDTH: f32 = 72.;
const USAGE_WIDTH: f32 = 46.;
/// Each of the time signature's two fields.
const TIME_SIGNATURE_FIELD_WIDTH: f32 = 22.;

pub struct TransportBar {
    bpm_input: NumberInput,
    /// The time signature's beats per bar.
    numerator_input: NumberInput,
    /// The time signature's note value.
    denominator_input: NumberInput,
}

impl TransportBar {
    pub fn new() -> Self {
        Self {
            bpm_input: NumberInput::new(Vec2::new(76., BUTTON_SIZE))
                .range(Rangef::new(10., 1000.))
                .suffix(" BPM")
                .draggable(),
            numerator_input: NumberInput::new(Vec2::new(TIME_SIGNATURE_FIELD_WIDTH, BUTTON_SIZE))
                .range(Rangef::new(1., TimeSignature::MAX_NUMERATOR as f32))
                .decimals(0)
                .draggable(),
            denominator_input: NumberInput::new(Vec2::new(TIME_SIGNATURE_FIELD_WIDTH, BUTTON_SIZE))
                .values(TimeSignature::DENOMINATORS.map(|d| d as f32))
                .decimals(0)
                .draggable(),
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        workspace: &Workspace,
        commands: &mut Commands,
    ) {
        egui::Panel::top("transport-bar")
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(ui.app_theme().bg_panel)
                    .inner_margin(Margin::same(4)),
            )
            .show(ui, |ui| {
                self.ui(ui, state, workspace, commands);
            });
    }

    /// Three groups: the views on the left, the transport centred, the
    /// project's tempo and the status on the right.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        workspace: &Workspace,
        commands: &mut Commands,
    ) {
        let full = ui.available_rect_before_wrap();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(BUTTON_GAP, BUTTON_GAP);
            if self.sidebar_ui(ui, state, workspace).clicked() {
                commands.push(Action::ToggleBrowser);
            }
            if self.graph_view_ui(ui, state, workspace).clicked() {
                commands.push(Action::ToggleGraphView);
            }

            let left = ui.min_rect().right();

            // The right group first: the transport keeps clear of it.
            let right = ui
                .with_layout(Layout::right_to_left(Align::Center), |ui| {
                    self.right_ui(ui, state, commands);
                    ui.min_rect().left()
                })
                .inner;

            // Centred in the bar, unless that would run into either side:
            // then as near the centre as fits, the time left out if even
            // that's too narrow.
            let room = right - left - 2. * GROUP_GAP;
            let buttons = if RECORDING { 5. } else { 4. };
            let buttons_width = buttons * (BUTTON_SIZE + BUTTON_GAP) + GROUP_GAP;
            let with_time = buttons_width + POSITION_WIDTH <= room;
            let position_width = if with_time {
                POSITION_WIDTH
            } else {
                POSITION_BARS_WIDTH
            };
            let width = buttons_width + position_width;
            let center_x = full
                .center()
                .x
                .min(right - GROUP_GAP - width / 2.)
                .max(left + GROUP_GAP + width / 2.);
            let center = Rect::from_center_size(
                pos2(center_x, ui.max_rect().center().y),
                vec2(width, BUTTON_SIZE),
            );
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(center)
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| {
                    // Under the right group when there's no room at all.
                    ui.set_clip_rect(ui.clip_rect().with_max_x(right - GROUP_GAP / 2.));
                    if self.metronome_ui(ui, state).clicked() {
                        commands.push(Action::ToggleMetronome);
                    }
                    if self.play_button_ui(ui, state).clicked() {
                        commands.push(Action::PlayStop);
                    }
                    if RECORDING {
                        self.record_button_ui(ui);
                    }
                    self.loop_ui(ui, state);
                    if self.follow_ui(ui, state).clicked() {
                        commands.push(Action::ToggleFollowPlayhead);
                    }
                    ui.add_space(GROUP_GAP - BUTTON_GAP);
                    position_ui(ui, state, with_time);
                },
            );
        });
    }

    /// Undo and redo, the status, the tempo and the time signature, laid
    /// out right to left.
    fn right_ui(&mut self, ui: &mut Ui, state: &mut ProjectState, commands: &mut Commands) {
        if self.redo_ui(ui, state).clicked() {
            commands.push(Action::Redo);
        }
        if self.undo_ui(ui, state).clicked() {
            commands.push(Action::Undo);
        }
        ui.add_space(GROUP_GAP);
        self.usage_ui(ui, state);
        if cfg!(debug_assertions) {
            self.fps_ui(ui);
        }
        if state.audio_lost() && self.no_audio_ui(ui, state).clicked() {
            commands.push(Action::OpenSettings);
        }
        ui.add_space(GROUP_GAP);
        self.bpm_input.value = state.bpm();
        self.bpm_input
            .ui(ui)
            .on_hover_text("Tempo: drag or double-click to edit");
        if self.bpm_input.value != state.bpm() {
            state.set_bpm(self.bpm_input.value);
        }
        ui.add_space(BUTTON_GAP);
        self.time_signature_ui(ui, state);
    }

    /// Beats per bar over the note that counts them, each typed or dragged.
    /// Laid out right to left, like the rest of this side of the bar.
    fn time_signature_ui(&mut self, ui: &mut Ui, state: &mut ProjectState) {
        let current = state.time_signature();
        self.denominator_input.value = current.denominator as f32;
        self.denominator_input
            .ui(ui)
            .on_hover_text("Note value of a beat: drag or double-click to edit");
        let denominator = self.denominator_input.value as u32;
        ui.label(RichText::new("/").color(ui.app_theme().text_muted));
        self.numerator_input.value = current.numerator as f32;
        self.numerator_input
            .ui(ui)
            .on_hover_text("Beats per bar: drag or double-click to edit");
        // Shift-dragging moves in tenths: only whole beats count.
        let numerator = self.numerator_input.value.round() as u32;
        if let Some(signature) = TimeSignature::new(numerator, denominator)
            && signature != current
        {
            state.set_time_signature(signature);
        }
    }

    fn play_button_ui(&mut self, ui: &mut Ui, state: &ProjectState) -> Response {
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
            FlatButton::new(if playback_state == PlaybackState::Playing {
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
            FlatButton::new(RECORD)
                .square(BUTTON_SIZE)
                .font(FontId::new(
                    14.,
                    egui::FontFamily::Name(PHOSPHOR_FILL.into()),
                ))
                .color(ui.app_theme().record)
                .tooltip("Record"),
        );
    }

    fn sidebar_ui(&mut self, ui: &mut Ui, state: &ProjectState, workspace: &Workspace) -> Response {
        ui.add(
            FlatButton::ghost(SIDEBAR_SIMPLE)
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
        )
    }

    fn graph_view_ui(
        &mut self,
        ui: &mut Ui,
        state: &ProjectState,
        workspace: &Workspace,
    ) -> Response {
        let active = workspace.main_view == MainView::Graph;
        ui.add(
            FlatButton::ghost(GRAPH)
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
        )
    }

    fn follow_ui(&mut self, ui: &mut Ui, state: &ProjectState) -> Response {
        let follow = state.follow_playhead();
        ui.add(
            FlatButton::new(egui_phosphor::fill::CARET_LINE_RIGHT)
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
        )
    }

    fn loop_ui(&mut self, ui: &mut Ui, state: &mut ProjectState) {
        let looping = state.looping();
        let res = ui.add(
            FlatButton::new(egui_phosphor::fill::REPEAT)
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

    fn metronome_ui(&mut self, ui: &mut Ui, state: &ProjectState) -> Response {
        let theme = ui.app_theme();
        let click = state.metronome()
            && matches!(state.playback_state(), PlaybackState::Playing)
            && state.playhead().0 % 1.0 < 0.5;

        ui.add(
            FlatButton::new(egui_phosphor::fill::METRONOME)
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
        )
    }

    fn usage_ui(&mut self, ui: &mut Ui, state: &mut ProjectState) -> Response {
        ui.add(
            FlatButton::new(format!(
                "{CPU} {:.0}%",
                (state.metrics.latency * 100.).round()
            ))
            .size(vec2(USAGE_WIDTH, BUTTON_SIZE))
            .font(FontId::new(
                font_size::SMALL,
                egui::FontFamily::Proportional,
            ))
            .color(ui.app_theme().text_muted)
            .tooltip("Audio processing load"),
        )
    }

    /// Shown while there's no audio output; opens the settings.
    fn no_audio_ui(&mut self, ui: &mut Ui, state: &ProjectState) -> Response {
        let error = state.audio_error.as_deref().unwrap_or("no device");
        ui.add(
            FlatButton::new(SPEAKER_SLASH)
                .square(BUTTON_SIZE)
                .color(ui.app_theme().danger)
                .tooltip(format!(
                    "No audio output ({error}). Retrying… Click to choose a device."
                )),
        )
    }

    fn fps_ui(&mut self, ui: &mut Ui) -> Response {
        ui.add(
            FlatButton::new(format!(
                "{:.0}",
                1.0 / ui.ctx().input(|i| i.stable_dt).max(1e-5)
            ))
            .square(BUTTON_SIZE)
            .font(FontId::new(
                font_size::SMALL,
                egui::FontFamily::Proportional,
            ))
            .color(ui.app_theme().text_muted)
            .tooltip("FPS"),
        )
    }

    fn undo_ui(&mut self, ui: &mut Ui, state: &mut ProjectState) -> Response {
        ui.add_enabled(
            state.can_undo(),
            FlatButton::ghost(egui_phosphor::fill::ARROW_U_UP_LEFT)
                .square(BUTTON_SIZE)
                .font(FontId::new(
                    15.,
                    egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                ))
                .tooltip(tooltip(ui, state, "Undo", Action::Undo)),
        )
    }

    fn redo_ui(&mut self, ui: &mut Ui, state: &mut ProjectState) -> Response {
        ui.add_enabled(
            state.can_redo(),
            FlatButton::ghost(egui_phosphor::fill::ARROW_U_UP_RIGHT)
                .square(BUTTON_SIZE)
                .font(FontId::new(
                    15.,
                    egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                ))
                .tooltip(tooltip(ui, state, "Redo", Action::Redo)),
        )
    }
}

/// Where the playhead is, in bars.beats.sixteenths and, `with_time`, in
/// time.
fn position_ui(ui: &mut Ui, state: &ProjectState, with_time: bool) {
    let width = if with_time {
        POSITION_WIDTH
    } else {
        POSITION_BARS_WIDTH
    };
    let (rect, _) = ui.allocate_exact_size(vec2(width, BUTTON_SIZE), Sense::hover());
    let theme = ui.app_theme();
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme.bg_deep);
    let (bars, time) = position_text(state.playhead().0, state.time_signature(), state.bpm());
    let font = FontId::proportional(font_size::BODY);
    painter.text(
        rect.left_center() + vec2(8., 0.),
        Align2::LEFT_CENTER,
        bars,
        font.clone(),
        theme.text,
    );
    if !with_time {
        return;
    }
    painter.text(
        rect.right_center() - vec2(8., 0.),
        Align2::RIGHT_CENTER,
        time,
        font,
        theme.text_muted,
    );
}

/// `beats` as `bar.beat.sixteenth` (from 1, beats as the signature counts
/// them) and `m:ss.mmm`.
fn position_text(beats: f64, signature: TimeSignature, bpm: f32) -> (String, String) {
    let beats = beats.max(0.);
    (
        format_bars(beats, signature),
        format_time(beats * 60. / bpm as f64),
    )
}

/// `text` with the shortcut of `action`, e.g. `Loop (Ctrl+L)`.
fn tooltip(ui: &Ui, state: &ProjectState, text: &str, action: Action) -> String {
    state
        .settings()
        .keymap
        .with_shortcut(ui.ctx(), text, action)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signature(numerator: u32, denominator: u32) -> TimeSignature {
        TimeSignature::new(numerator, denominator).unwrap()
    }

    #[test]
    fn position_counts_from_one() {
        let four = signature(4, 4);
        assert_eq!(
            position_text(0., four, 120.),
            ("1.1.1".to_string(), "0:00.000".to_string())
        );
        // Bar 2, beat 3, the second sixteenth: 6.25 beats, 3.125 s at 120.
        assert_eq!(
            position_text(6.25, four, 120.),
            ("2.3.2".to_string(), "0:03.125".to_string())
        );
        assert_eq!(position_text(150., four, 60.).1, "2:30.000");
    }

    /// Beats are counted in the signature's note value.
    #[test]
    fn position_follows_the_time_signature() {
        // 3/4: beat 6.25 is in bar 3, first beat, second sixteenth.
        assert_eq!(position_text(6.25, signature(3, 4), 120.).0, "3.1.2");
        // 6/8: a bar is 3 quarters of 6 eighths; 4.75 is bar 2, 4th eighth.
        assert_eq!(position_text(4.75, signature(6, 8), 120.).0, "2.4.2");
        // 7/8: bar 2 starts at 3.5.
        assert_eq!(position_text(3.5, signature(7, 8), 120.).0, "2.1.1");
    }
}
