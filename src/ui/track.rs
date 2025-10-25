use crate::{
    core::{
        state::ToniqueProjectState,
        track::{MutableTrackCore, TrackCore, TrackReferenceCore},
    },
    ui::widget::{
        context_menu::{ContextMenuButton, ContextMenuSeparator},
        meter::LoudnessMeter as Meter,
        rectangle::Rectangle,
        square_button::SquareButton,
    },
    utils::parse_name,
};
use egui::{
    Align2, Color32, FontId, Frame, Label, Margin, Pos2, Rect, Response, RichText, Sense, Stroke,
    TextEdit, Ui, Vec2, epaint::MarginF32,
};
use egui_phosphor::fill::{CIRCLE, COPY, CURSOR_TEXT, PALETTE, PLUS, TRASH};
use rand::Rng;
use std::ops::RangeInclusive;

pub const DEFAULT_TRACK_HEIGHT: f32 = 60.;
const STROKE_WIDTH: f32 = 0.5;
const PADDING: f32 = 2.;
const BUTTON_SIZE: f32 = 15.;
pub const CLOSED_HEIGHT: f32 = 22.;
const METER_WIDTH: f32 = 8.;
pub const HANDLE_HEIGHT: f32 = 3.0;

#[derive(Debug, Clone)]
pub struct UITrack {
    gain: f32,
    old_volume: f32,
    prev_height: f32,
    edit: bool,
    arm: bool,
    _edit_lost_focus: bool,
}

impl UITrack {
    pub fn new() -> Self {
        Self {
            _edit_lost_focus: false,
            arm: false,
            edit: false,
            gain: 0.,
            old_volume: 1.0,
            prev_height: DEFAULT_TRACK_HEIGHT,
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        track: &TrackReferenceCore,
        state: &mut ToniqueProjectState,
    ) -> Response {
        // Create persisant id
        let id = ui.make_persistent_id(format!("ui_track_state_{}", track.id));
        // Get previous state
        if let Some(data) = ui.ctx().data(|r| r.get_temp::<Self>(id)) {
            *self = data;
        };

        let mut volume_changed = false;
        let mut double_clicked = false;
        let muted = track.disabled();
        let is_solo = matches!(track.solo, crate::core::track::TrackSoloState::Solo);

        let main_frame = Frame::new()
            .inner_margin(MarginF32::same(PADDING))
            .fill(if track.selected {
                Color32::from_gray(30)
            } else {
                Color32::from_gray(40)
            })
            .stroke(Stroke::new(STROKE_WIDTH, Color32::from_gray(70)));

        let frame_res = main_frame.show(ui, |ui| {
            let actual_height = track.height - 2. * PADDING - 2. * STROKE_WIDTH;
            ui.set_width(ui.available_width());
            ui.set_height(actual_height);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
                // Left Side: Rectangle
                ui.add(
                    Rectangle::new(Vec2::new(4., actual_height)).fill(if !muted {
                        track.color
                    } else {
                        Color32::from_gray(100)
                    }),
                );
                let response = ui.interact(
                    Rect::from_min_size(
                        ui.next_widget_position(),
                        Vec2::new(
                            ui.available_width(),
                            track.height - 2. * PADDING - 2. * STROKE_WIDTH,
                        ),
                    ),
                    ui.make_persistent_id(format!("track-{}", track.id)),
                    Sense::click_and_drag(),
                );

                if response.clicked() || response.dragged() {
                    state.select_track(&track.id);
                };
                double_clicked = response.double_clicked();
                // Middle: Text & Controls
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.set_height(BUTTON_SIZE);

                        // let track_mut = state.track_mut(&track.id);
                        self.open_button(ui, track, state);

                        let text_width =
                            ui.available_width() - 4. * PADDING - 3. * BUTTON_SIZE - METER_WIDTH;
                        // Track label
                        if text_width > 0. {
                            ui.scope(|ui| {
                                ui.set_width(text_width);
                                self.text_ui(ui, track, state);
                            });
                        }
                        // Track controls
                        let mute_res = self.mute_button(ui, is_solo, track);
                        let solo_res = self.solo_button(ui, is_solo, track);
                        let arm_res = self.arm_button(ui);

                        if mute_res.clicked() {
                            state.set_mute(track.id.clone(), !track.muted);
                        }
                        if solo_res.clicked() {
                            state.toggle_solo(track.id.clone(), ui.input(|i| i.modifiers.shift));
                        }
                        if arm_res.clicked() {
                            self.arm = !self.arm;
                        }
                    });
                    let track_mut = state.track_mut(&track.id);
                    // Extra controls
                    if !track_mut.closed {
                        let prev_gain = self.gain;
                        self.gain_slider(ui, RangeInclusive::new(-40., 5.), track, state);
                        volume_changed = prev_gain != self.gain;
                    };
                });

                // Right side: Meter
                if let Some(metrics) = state.metrics.tracks.get_mut(&track.id) {
                    ui.vertical(|ui| {
                        ui.add_sized(
                            Vec2::new(6.0, ui.available_height()),
                            Meter::new(Vec2::new(METER_WIDTH, actual_height), metrics.clone())
                                .disabled(muted),
                        );
                    });
                }
                response.context_menu(|ui| {
                    self.context_menu(ui, track, state);
                });
            });
        });
        // Drag area
        self.dragger(ui, track, state);

        // Save temporary state
        ui.data_mut(|w| w.insert_temp(id, self.clone()));
        frame_res.response
    }

    fn text_ui(
        &mut self,
        ui: &mut Ui,
        track: &TrackReferenceCore,
        state: &mut ToniqueProjectState,
    ) {
        if self.edit {
            let track_mut = state.track_mut(&track.id);
            let text_edit = ui.add(
                TextEdit::singleline(&mut track_mut.name)
                    .font(FontId::new(9., egui::FontFamily::Proportional))
                    .background_color(Color32::from_black_alpha(20))
                    .text_color(Color32::WHITE)
                    .margin(Margin::ZERO),
            );
            if !text_edit.has_focus() && !self._edit_lost_focus {
                text_edit.request_focus();
                self._edit_lost_focus = true
            }
            if text_edit.lost_focus() {
                self.edit = false;
                self._edit_lost_focus = false;
                if track_mut.name.is_empty() {
                    track_mut.name = "Audio Track".to_string();
                }
                state.commit_track_mut(&track.id);
            }
        } else {
            let formatted_name = parse_name(&track.name, track.index);
            ui.add(
                Label::new(RichText::new(formatted_name).color(Color32::WHITE).size(9.))
                    .truncate()
                    .selectable(false),
            );
        }
    }

    fn context_menu(
        &mut self,
        ui: &mut Ui,
        track: &TrackReferenceCore,
        state: &mut ToniqueProjectState,
    ) {
        Frame::new().show(ui, |ui| {
            ui.vertical(|ui| {
                if ui
                    .add(ContextMenuButton::new(CURSOR_TEXT, "Rename"))
                    .clicked()
                {
                    self.edit = true;
                };
                if ui
                    .add(ContextMenuButton::new(PLUS, "Add Audio Track"))
                    .clicked()
                {
                    state.add_track_at(TrackCore::new(), track.index);
                }
                if ui.add(ContextMenuButton::new(COPY, "Duplicate")).clicked() {
                    state.duplicate_track(&track.id);
                };
                if ui
                    .add(ContextMenuButton::new(PALETTE, "Change Color"))
                    .clicked()
                {
                    let mut rng = rand::rng();
                    let track_mut = state.track_mut(&track.id);
                    track_mut.color = Color32::from_rgb(
                        rng.random_range(0..=255),
                        rng.random_range(0..=255),
                        rng.random_range(0..=255),
                    );
                    state.commit_track_mut(&track.id);
                }
                ui.add(ContextMenuSeparator::new());
                if ui
                    .add(ContextMenuButton::new(TRASH, "Delete").text_color(Color32::LIGHT_RED))
                    .clicked()
                {
                    state.delete_track(&track.id);
                };
            });
        });
    }

    fn mute_button(&mut self, ui: &mut Ui, solo: bool, track: &TrackReferenceCore) -> Response {
        ui.add(
            SquareButton::new("M")
                .sized(BUTTON_SIZE)
                .fill(if track.muted {
                    if solo {
                        Color32::from_rgb(60, 40, 20)
                    } else {
                        Color32::from_rgb(191, 74, 15)
                    }
                } else {
                    ui.visuals().widgets.inactive.bg_fill
                }),
        )
    }

    fn solo_button(&mut self, ui: &mut Ui, solo: bool, track: &TrackReferenceCore) -> Response {
        ui.add(SquareButton::new("S").sized(BUTTON_SIZE).fill(if solo {
            track.color
        } else {
            ui.visuals().widgets.inactive.bg_fill
        }))
    }

    fn arm_button(&mut self, ui: &mut Ui) -> Response {
        ui.add(
            SquareButton::new(CIRCLE)
                .sized(BUTTON_SIZE)
                .fill(if self.arm {
                    Color32::from_rgb(220, 30, 30)
                } else {
                    ui.visuals().widgets.inactive.bg_fill
                }),
        )
    }

    fn open_button(
        &mut self,
        ui: &mut Ui,
        track: &TrackReferenceCore,
        state: &mut ToniqueProjectState,
    ) -> Response {
        let track_mut = state.track_mut(&track.id);
        let icon = if track_mut.closed {
            egui_phosphor::fill::CARET_RIGHT
        } else {
            egui_phosphor::fill::CARET_DOWN
        };
        let response = ui.add(
            SquareButton::new(icon)
                .fill(Color32::from_gray(80))
                .sized(BUTTON_SIZE),
        );
        if response.clicked() {
            self.toggle_open(track_mut);
        }

        response
    }

    fn toggle_open(&mut self, track_mut: &mut MutableTrackCore) {
        track_mut.closed = !track_mut.closed;
        if track_mut.closed {
            track_mut.height = CLOSED_HEIGHT;
        } else {
            track_mut.height = self.prev_height;
        }
    }

    fn gain_slider(
        &mut self,
        ui: &mut Ui,
        range: std::ops::RangeInclusive<f32>,
        track: &TrackReferenceCore,
        state: &mut ToniqueProjectState,
    ) -> Response {
        let desired_size = egui::vec2(2. * BUTTON_SIZE + 1., 20.);
        let (rect, mut response) = ui.allocate_exact_size(desired_size, Sense::click_and_drag());
        self.gain = 20. * track.volume.log10();

        if response.dragged() {
            let delta = response.drag_delta().x;
            self.gain += delta * (range.end() - range.start()) / rect.width();
            self.gain = self.gain.clamp(*range.start(), *range.end());
            state.set_volume(track.id.clone(), 10f32.powf(self.gain / 20.));
            response.mark_changed();
        }

        if response.drag_stopped() {
            let new_volume = 10f32.powf(self.gain / 20.);
            state.commit_volume(track.id.clone(), self.old_volume, new_volume);
            self.old_volume = new_volume;
        }

        if response.double_clicked() {
            self.gain = 0.;
            state.commit_volume(track.id.clone(), self.old_volume, 1.0);
            self.old_volume = 1.0;
            response.mark_changed();
        }

        if response.hovered() {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
        }

        // Compute fill ratio
        let t = (self.gain - *range.start()) / (*range.end() - *range.start());

        // Paint background bar
        let visuals = ui.style().visuals.clone();
        let bg_fill = visuals.extreme_bg_color;
        let fill_color = track.color;

        let painter = ui.painter();

        // Full background
        painter.rect_filled(rect, 1.0, bg_fill);

        // Filled ratio bar
        let fill_rect = Rect::from_min_max(
            rect.min,
            Pos2::new(rect.left() + rect.width() * t, rect.bottom()),
        );
        painter.rect_filled(fill_rect, 2.0, fill_color);

        // Text value
        let text = format!("{:.1}", self.gain);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::new(10., egui::FontFamily::Proportional),
            Color32::WHITE,
        );

        response
    }

    fn dragger(
        &mut self,
        ui: &mut Ui,
        track: &TrackReferenceCore,
        state: &mut ToniqueProjectState,
    ) {
        let track_mut = state.track_mut(&track.id);
        let (_, mut response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), HANDLE_HEIGHT),
            Sense::drag(),
        );

        if !track_mut.closed {
            response = response.on_hover_cursor(egui::CursorIcon::ResizeVertical);
        }
        if response.dragged() && !track_mut.closed {
            track_mut.height += response.drag_delta().y;
            track_mut.height = track_mut.height.clamp(CLOSED_HEIGHT + 25., 400.);
            self.prev_height = track_mut.height;
        }
    }
}
