use crate::ui::theme::font_size;
use crate::{
    core::{
        state::{MASTER_TRACK_ID, MIN_EXPANDED_HEIGHT, ProjectState},
        track::{TrackKind, TrackRow},
    },
    ui::{
        RECORDING,
        font::PHOSPHOR_FILL,
        theme::{Theme, ThemeExt, with_alpha},
        widget::{
            color_bar::ColorBar,
            color_select::ColorSelect,
            context_menu::{ContextMenuButton, ContextMenuLabel, ContextMenuSeparator},
            flat_button::FlatButton,
            meter::LevelMeter,
            param_slider::{ParamSlider, SliderEdit},
        },
    },
    utils::display_name,
};
use egui::{
    Color32, FontId, Frame, Label, Margin, Rect, Response, RichText, Sense, Stroke, TextEdit, Ui,
    Vec2, epaint::MarginF32, vec2,
};
use egui_phosphor::{
    fill::{COPY, FOLDER_MINUS, FOLDER_PLUS, PALETTE, PLUS, TRASH},
    regular::{MUSIC_NOTE_SIMPLE, TEXT_T},
};
use std::ops::RangeInclusive;

const STROKE_WIDTH: f32 = 0.5;
const PADDING: f32 = 2.;
const BUTTON_SIZE: f32 = 15.;
const METER_WIDTH: f32 = 8.;
pub const ROW_GAP: f32 = 3.0;
/// Indentation per level of groups, up to [`MAX_INDENT_LEVELS`] (deeper
/// rows line up with the deepest shown level).
const INDENT: f32 = 8.;
const MAX_INDENT_LEVELS: usize = 6;
/// Width of the coloured bar on the left of each header.
pub const COLOR_BAR_WIDTH: f32 = 4.;
/// Volume slider range, in dB: its bottom is silence.
pub const VOLUME_RANGE: RangeInclusive<f32> = -40.0..=5.0;
const SLIDER_SIZE: Vec2 = vec2(2. * BUTTON_SIZE + 1., 20.);
/// Gap between the rows of controls.
const CONTROLS_GAP: f32 = 4.;

/// Left of the coloured bar of a row at `depth`, in a header starting at
/// `left`. Groups extend theirs down to their last row from there.
pub fn color_bar_x(left: f32, depth: usize) -> f32 {
    left + STROKE_WIDTH + PADDING + depth.min(MAX_INDENT_LEVELS) as f32 * INDENT
}

/// Space between a header's edge and its content, top and bottom.
pub const HEADER_INSET: f32 = STROKE_WIDTH + PADDING;

#[derive(Debug, Clone)]
pub struct TrackHeader {
    edit: bool,
    focus_requested: bool,
}

impl TrackHeader {
    pub fn new() -> Self {
        Self {
            focus_requested: false,
            edit: false,
        }
    }

    /// The header of `track`; `rename` starts typing its name.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        track: &TrackRow,
        state: &mut ProjectState,
        rename: bool,
    ) -> Response {
        // Create persistent id
        let id = ui.make_persistent_id(format!("ui_track_state_{:?}", track.id));
        // Get previous state
        if let Some(data) = ui.ctx().data(|r| r.get_temp::<Self>(id)) {
            *self = data;
        };
        if rename {
            self.edit = true;
            self.focus_requested = false;
        }

        let is_group = track.kind == TrackKind::Group;
        let silenced = track.is_silenced();
        let is_solo = matches!(track.solo, crate::core::track::TrackSoloState::Solo);

        let theme = ui.app_theme();
        let main_frame = Frame::new()
            .inner_margin(MarginF32::same(PADDING))
            .fill(if track.selected {
                theme.bg_control
            } else {
                theme.bg_raised
            })
            .stroke(Stroke::new(STROKE_WIDTH, theme.separator));

        let res = main_frame
            .show(ui, |ui| {
                let actual_height = track.height - 2. * PADDING - 2. * STROKE_WIDTH;
                ui.set_width(ui.available_width());
                ui.set_height(actual_height);
                ui.spacing_mut().interact_size.y = 18.0;
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);

                    // Indentation: the bars of the groups it's in run here (see
                    // `color_bar_x`).
                    let levels = track.depth.min(MAX_INDENT_LEVELS);
                    if levels > 0 {
                        ui.add_space(levels as f32 * INDENT);
                    }

                    // Left Side: ColorBar
                    ui.add(
                        ColorBar::new(Vec2::new(COLOR_BAR_WIDTH, actual_height)).color(
                            if !silenced {
                                track.color
                            } else {
                                theme.text_disabled
                            },
                        ),
                    );
                    let response = ui.interact(
                        Rect::from_min_size(
                            ui.next_widget_position(),
                            Vec2::new(
                                ui.available_width(),
                                track.height - 2. * PADDING - 2. * STROKE_WIDTH,
                            ),
                        ),
                        ui.make_persistent_id(format!("track-{:?}", track.id)),
                        Sense::click_and_drag(),
                    );

                    // Ctrl+click adds to the selection, Shift+click selects a range.
                    if response.clicked() {
                        let modifiers = ui.input(|i| i.modifiers);
                        if modifiers.command {
                            state.toggle_track_selected(track.id);
                        } else if modifiers.shift {
                            state.select_rows_to(track.id);
                        } else {
                            state.select_track(&track.id);
                        }
                    } else if response.drag_started() && !track.selected {
                        state.select_track(&track.id);
                    }

                    // Middle: Text & Controls
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = CONTROLS_GAP;
                        ui.horizontal(|ui| {
                            ui.set_height(BUTTON_SIZE);

                            self.collapse_button(ui, track, state);

                            // Groups and the master have no arm button.
                            let armable = RECORDING && !is_group && track.id != MASTER_TRACK_ID;
                            let buttons = if armable { 3. } else { 2. };
                            let text_width = ui.available_width()
                                - 4. * PADDING
                                - buttons * BUTTON_SIZE
                                - METER_WIDTH;
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
                            let arm_res = armable.then(|| self.arm_button(ui, track));

                            if mute_res.clicked() {
                                state.set_mute(track.id, !track.muted);
                            }
                            if solo_res.clicked() {
                                state.toggle_solo(track.id, ui.input(|i| i.modifiers.shift));
                            }
                            if arm_res.is_some_and(|r| r.clicked()) {
                                state.set_armed(&track.id, !track.armed);
                            }
                        });
                        // Volume and pan
                        if !track.collapsed {
                            ui.horizontal(|ui| {
                                volume_slider(ui, track, state);
                                pan_slider(ui, track, state);
                            });
                        };
                    });

                    // Right side: Meter
                    if let Some(metrics) = state.metrics.tracks.get_mut(&track.id) {
                        ui.vertical(|ui| {
                            ui.add_sized(
                                Vec2::new(6.0, ui.available_height()),
                                LevelMeter::new(
                                    Vec2::new(METER_WIDTH, actual_height),
                                    metrics.clone(),
                                )
                                .disabled(silenced),
                            );
                        });
                    }
                    response.context_menu(|ui| {
                        self.context_menu(ui, track, state);
                    });

                    response
                })
                .inner
            })
            .inner;
        // Drag area
        self.resize_handle(ui, track, state);

        // Save temporary state
        ui.data_mut(|w| w.insert_temp(id, self.clone()));
        res
    }

    fn text_ui(&mut self, ui: &mut Ui, track: &TrackRow, state: &mut ProjectState) {
        if self.edit {
            let track_view_mut = state.track_view_mut(&track.id);
            let text_edit = ui.add(
                TextEdit::singleline(&mut track_view_mut.name)
                    .font(FontId::new(
                        font_size::SMALL,
                        egui::FontFamily::Proportional,
                    ))
                    .background_color(ui.app_theme().bg_deep)
                    .text_color(ui.app_theme().text)
                    .margin(Margin::ZERO),
            );
            if !text_edit.has_focus() && !self.focus_requested {
                text_edit.request_focus();
                self.focus_requested = true
            }
            if text_edit.lost_focus() {
                self.edit = false;
                self.focus_requested = false;
                if track_view_mut.name.is_empty() {
                    track_view_mut.name = "Audio Track".to_string();
                }
                state.commit_track_view(&track.id);
            }
        } else {
            name_label(ui, track);
        }
    }

    fn context_menu(&mut self, ui: &mut Ui, track: &TrackRow, state: &mut ProjectState) {
        let is_group = track.kind == TrackKind::Group;
        Frame::new().show(ui, |ui| {
            ui.vertical(|ui| {
                ui.add(ContextMenuLabel::new(display_name(
                    &track.name,
                    track.first_track_index,
                )));
                if ui.add(ContextMenuButton::new(TEXT_T, "Rename")).clicked() {
                    self.edit = true;
                };
                if ui
                    .add(ContextMenuButton::new(PLUS, "Add Audio Track"))
                    .clicked()
                {
                    state.add_track_at(track.first_track_index);
                }
                // The whole selection when this row is part of it.
                let selection = state.selected_tracks().clone();
                let targets = if selection.contains(&track.id) {
                    selection
                } else {
                    vec![track.id]
                };
                if ui
                    .add(ContextMenuButton::new(FOLDER_PLUS, "Group"))
                    .clicked()
                {
                    state.group(&targets);
                }
                if is_group {
                    if ui
                        .add(ContextMenuButton::new(FOLDER_MINUS, "Ungroup"))
                        .clicked()
                    {
                        state.ungroup(track.id);
                    }
                } else if ui.add(ContextMenuButton::new(COPY, "Duplicate")).clicked() {
                    state.duplicate_track(&track.id);
                };
                ContextMenuButton::new(PALETTE, "Color").submenu(ui, |ui| {
                    let mut color = track.color;
                    let picker = ColorSelect::new(("track-color", track.id), &mut color);
                    if ui.add(picker).changed() {
                        state.track_view_mut(&track.id).color = color;
                        state.commit_track_view(&track.id);
                    }
                });
                ui.add(ContextMenuSeparator::new());
                let delete = if is_group {
                    "Delete group and contents"
                } else {
                    "Delete"
                };
                if ui
                    .add(ContextMenuButton::new(TRASH, delete).text_color(ui.app_theme().danger))
                    .clicked()
                {
                    state.delete_track(&track.id);
                };
            });
        });
    }

    fn mute_button(&mut self, ui: &mut Ui, solo: bool, track: &TrackRow) -> Response {
        let theme = ui.app_theme();
        // Dimmed when a solo already silences the track.
        let fill = match (track.muted, solo) {
            (false, _) => None,
            (true, false) => Some(theme.warning),
            (true, true) => Some(with_alpha(theme.warning, 90)),
        };
        ui.add(toggle_button("M", fill, &theme))
    }

    fn solo_button(&mut self, ui: &mut Ui, solo: bool, track: &TrackRow) -> Response {
        let theme = ui.app_theme();
        ui.add(toggle_button("S", solo.then_some(track.color), &theme))
    }

    fn arm_button(&mut self, ui: &mut Ui, track: &TrackRow) -> Response {
        let theme = ui.app_theme();
        ui.add(toggle_button(
            MUSIC_NOTE_SIMPLE,
            track.armed.then_some(theme.record),
            &theme,
        ))
    }

    fn collapse_button(
        &mut self,
        ui: &mut Ui,
        track: &TrackRow,
        state: &mut ProjectState,
    ) -> Response {
        let collapsed = state.track_view_mut(&track.id).collapsed;
        let response = ui.add(collapse_button(collapsed));
        if response.clicked() {
            state.set_collapsed(&track.id, !collapsed);
        }

        response
    }

    fn resize_handle(&mut self, ui: &mut Ui, track: &TrackRow, state: &mut ProjectState) {
        let track_view_mut = state.track_view_mut(&track.id);
        let (_, mut response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_GAP), Sense::drag());

        if !track_view_mut.collapsed {
            response = response.on_hover_cursor(egui::CursorIcon::ResizeVertical);
        }
        if response.dragged() && !track_view_mut.collapsed {
            track_view_mut.height += response.drag_delta().y;
            track_view_mut.height = track_view_mut.height.clamp(MIN_EXPANDED_HEIGHT, 400.);
        }
    }
}

/// Volume in dB, filled from the left.
fn volume_slider(ui: &mut Ui, track: &TrackRow, state: &mut ProjectState) {
    let slider = ParamSlider::gain(track.volume, VOLUME_RANGE)
        .color(track.color)
        .size(SLIDER_SIZE);
    match slider.show(ui).1 {
        SliderEdit::Moved(volume) => state.set_volume(track.id, volume),
        SliderEdit::Released { start, value } if value != start => {
            state.commit_volume(track.id, start, value)
        }
        SliderEdit::Reset { from, to } => state.commit_volume(track.id, from, to),
        _ => {}
    }
}

/// Pan, filled from the centre towards the side it leans to.
fn pan_slider(ui: &mut Ui, track: &TrackRow, state: &mut ProjectState) {
    let slider = ParamSlider::pan(track.pan)
        .color(track.color)
        .size(SLIDER_SIZE);
    match slider.show(ui).1 {
        SliderEdit::Moved(pan) => state.set_pan(track.id, pan),
        SliderEdit::Released { start, value } if value != start => {
            state.commit_pan(track.id, start, value)
        }
        SliderEdit::Reset { from, to } => state.commit_pan(track.id, from, to),
        _ => {}
    }
}

/// A header as track `track` will have once added, faded and inert: for a
/// track a drop would add. The same widgets as [`TrackHeader`], in the same
/// places, so the real one takes its place without a jump.
pub fn ghost_header(ui: &mut Ui, track: &TrackRow) {
    let theme = ui.app_theme();
    // Opaque under the fading, as the real one is.
    ui.painter().rect_filled(
        Rect::from_min_size(
            ui.next_widget_position(),
            vec2(ui.available_width(), track.height),
        ),
        0.,
        theme.bg_raised,
    );
    ui.disable();
    Frame::new()
        .inner_margin(MarginF32::same(PADDING))
        .fill(theme.bg_raised)
        .stroke(Stroke::new(STROKE_WIDTH, theme.separator))
        .show(ui, |ui| {
            let actual_height = track.height - 2. * PADDING - 2. * STROKE_WIDTH;
            ui.set_width(ui.available_width());
            ui.set_height(actual_height);
            ui.spacing_mut().interact_size.y = 18.0;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
                ui.add(ColorBar::new(Vec2::new(COLOR_BAR_WIDTH, actual_height)).color(track.color));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = CONTROLS_GAP;
                    ui.horizontal(|ui| {
                        ui.set_height(BUTTON_SIZE);
                        ui.add(collapse_button(false));
                        let buttons = if RECORDING { 3. } else { 2. };
                        let text_width = ui.available_width()
                            - 4. * PADDING
                            - buttons * BUTTON_SIZE
                            - METER_WIDTH;
                        if text_width > 0. {
                            ui.scope(|ui| {
                                ui.set_width(text_width);
                                name_label(ui, track);
                            });
                        }
                        ui.add(toggle_button("M", None, &theme));
                        ui.add(toggle_button("S", None, &theme));
                        if RECORDING {
                            ui.add(toggle_button(MUSIC_NOTE_SIMPLE, None, &theme));
                        }
                    });
                    ui.horizontal(|ui| {
                        ParamSlider::gain(track.volume, VOLUME_RANGE)
                            .color(track.color)
                            .size(SLIDER_SIZE)
                            .show(ui);
                        ParamSlider::pan(track.pan)
                            .color(track.color)
                            .size(SLIDER_SIZE)
                            .show(ui);
                    });
                });
            });
        });
}

/// The track's name, as shown when not renaming it.
fn name_label(ui: &mut Ui, track: &TrackRow) {
    ui.add(
        Label::new(
            RichText::new(display_name(&track.name, track.first_track_index))
                .color(ui.app_theme().text)
                .size(font_size::SMALL),
        )
        .truncate()
        .selectable(false),
    );
}

/// The caret folding a row: pointing right when `collapsed`.
fn collapse_button(collapsed: bool) -> FlatButton {
    let icon = if collapsed {
        egui_phosphor::fill::CARET_RIGHT
    } else {
        egui_phosphor::fill::CARET_DOWN
    };
    FlatButton::new(icon)
        .family(egui::FontFamily::Name(PHOSPHOR_FILL.into()))
        .square(BUTTON_SIZE)
}

/// Track header toggle: `fill` when on, the default control colour when off.
fn toggle_button(text: &str, fill: Option<Color32>, theme: &Theme) -> FlatButton {
    let button = FlatButton::new(text).square(BUTTON_SIZE);
    match fill {
        Some(fill) => button.fill(fill).color(theme.text_on(fill)),
        None => button,
    }
}
