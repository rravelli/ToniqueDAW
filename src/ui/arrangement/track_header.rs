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
        },
    },
    utils::display_name,
};
use egui::{
    Align2, Color32, FontId, Frame, Label, Margin, Rect, Response, RichText, Sense, Stroke,
    TextEdit, Ui, Vec2, epaint::MarginF32, pos2, vec2,
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
/// Shift-dragging a slider moves it this much slower.
const FINE_DRAG: f32 = 0.1;
/// Volume slider range, in dB.
const GAIN_RANGE: RangeInclusive<f32> = -40.0..=5.0;
const PAN_RANGE: RangeInclusive<f32> = -1.0..=1.0;
/// Pan closer to the centre than this snaps to it while dragging.
const PAN_SNAP: f32 = 0.02;
const SLIDER_WIDTH: f32 = 2. * BUTTON_SIZE + 1.;
const SLIDER_HEIGHT: f32 = 20.;
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
    gain_db: f32,
    /// The volume or pan drag in progress.
    gain_drag: Option<ParamDrag>,
    pan_drag: Option<ParamDrag>,
    edit: bool,
    focus_requested: bool,
}

impl TrackHeader {
    pub fn new() -> Self {
        Self {
            focus_requested: false,
            edit: false,
            gain_db: 0.,
            gain_drag: None,
            pan_drag: None,
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
                                self.volume_slider(ui, SLIDER_WIDTH, track, state);
                                self.pan_slider(ui, SLIDER_WIDTH, track, state);
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
            let formatted_name = display_name(&track.name, track.first_track_index);
            ui.add(
                Label::new(
                    RichText::new(formatted_name)
                        .color(ui.app_theme().text)
                        .size(font_size::SMALL),
                )
                .truncate()
                .selectable(false),
            );
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
        let track_view_mut = state.track_view_mut(&track.id);
        let icon = if track_view_mut.collapsed {
            egui_phosphor::fill::CARET_RIGHT
        } else {
            egui_phosphor::fill::CARET_DOWN
        };
        let response = ui.add(
            FlatButton::new(icon)
                .family(egui::FontFamily::Name(PHOSPHOR_FILL.into()))
                .square(BUTTON_SIZE),
        );
        let collapsed = track_view_mut.collapsed;
        if response.clicked() {
            state.set_collapsed(&track.id, !collapsed);
        }

        response
    }

    /// Volume in dB, filled from the left.
    fn volume_slider(
        &mut self,
        ui: &mut Ui,
        width: f32,
        track: &TrackRow,
        state: &mut ProjectState,
    ) -> Response {
        let (rect, mut response) = slider_rect(ui, width);
        let range = GAIN_RANGE;
        self.gain_db = 20. * track.volume.log10();

        match anchored_drag(
            ui,
            &response,
            &mut self.gain_drag,
            self.gain_db,
            track.volume,
            &range,
        ) {
            SliderDrag::Moved(gain) => {
                self.gain_db = gain;
                state.set_volume(track.id, db_to_volume(gain));
                response.mark_changed();
            }
            SliderDrag::Released { start } => {
                let volume = db_to_volume(self.gain_db);
                if volume != start {
                    state.commit_volume(track.id, start, volume);
                }
            }
            SliderDrag::None => {}
        }
        if response.double_clicked() && track.volume != 1. {
            self.gain_db = 0.;
            state.commit_volume(track.id, track.volume, 1.0);
            response.mark_changed();
        }

        let t = ((self.gain_db - range.start()) / (range.end() - range.start())).clamp(0., 1.);
        let fill = Rect::from_min_max(rect.min, pos2(rect.lerp_inside(vec2(t, 0.)).x, rect.max.y));
        paint_slider(ui, rect, fill, track.color, &format_gain(self.gain_db));
        response
    }

    /// Pan, filled from the centre towards the side it leans to.
    fn pan_slider(
        &mut self,
        ui: &mut Ui,
        width: f32,
        track: &TrackRow,
        state: &mut ProjectState,
    ) -> Response {
        let (rect, mut response) = slider_rect(ui, width);
        let mut pan = track.pan;

        match anchored_drag(
            ui,
            &response,
            &mut self.pan_drag,
            pan,
            track.pan,
            &PAN_RANGE,
        ) {
            SliderDrag::Moved(value) => {
                pan = snap_to_centre(value);
                state.set_pan(track.id, pan);
                response.mark_changed();
            }
            SliderDrag::Released { start } => {
                if pan != start {
                    state.commit_pan(track.id, start, pan);
                }
            }
            SliderDrag::None => {}
        }
        if response.double_clicked() && track.pan != 0. {
            pan = 0.;
            state.commit_pan(track.id, track.pan, 0.);
            response.mark_changed();
        }

        let centre = rect.center().x;
        let x = rect.lerp_inside(vec2((pan + 1.) / 2., 0.)).x;
        let fill = Rect::from_x_y_ranges(centre.min(x)..=centre.max(x), rect.y_range());
        paint_slider(ui, rect, fill, track.color, &format_pan(pan));
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

/// A slider drag: the value follows the pointer's distance from where it
/// was anchored, so going past an end and back lands where it left.
#[derive(Debug, Clone, Copy)]
struct ParamDrag {
    /// The parameter's value when the drag started, for undo.
    start: f32,
    anchor_x: f32,
    /// Slider value at `anchor_x`.
    anchor_value: f32,
    /// Shift held: slower, for fine adjustment.
    fine: bool,
}

enum SliderDrag {
    None,
    /// Dragged to this slider value.
    Moved(f32),
    /// Released: commit the change from the parameter's `start` value.
    Released {
        start: f32,
    },
}

/// Drive `drag` from `response`. `value` is the slider's current value in
/// `range`, `param` the parameter's (they differ for volume: dB vs gain).
fn anchored_drag(
    ui: &Ui,
    response: &Response,
    drag: &mut Option<ParamDrag>,
    value: f32,
    param: f32,
    range: &RangeInclusive<f32>,
) -> SliderDrag {
    let fine = ui.input(|i| i.modifiers.shift);
    let pointer_x = response.interact_pointer_pos().map(|p| p.x);
    let value = value.clamp(*range.start(), *range.end());

    if response.drag_started()
        && let Some(x) = pointer_x
    {
        *drag = Some(ParamDrag {
            start: param,
            anchor_x: x,
            anchor_value: value,
            fine,
        });
    }
    if response.drag_stopped()
        && let Some(drag) = drag.take()
    {
        return SliderDrag::Released { start: drag.start };
    }
    if response.dragged()
        && let (Some(drag), Some(x)) = (drag.as_mut(), pointer_x)
    {
        // Toggling Shift re-anchors, so the value doesn't jump.
        if drag.fine != fine {
            *drag = ParamDrag {
                anchor_x: x,
                anchor_value: value,
                fine,
                ..*drag
            };
        }
        let dragged = dragged_value(drag, x, response.rect.width(), range);
        if dragged != value {
            return SliderDrag::Moved(dragged);
        }
    }
    SliderDrag::None
}

/// Slider value with the pointer at `x`, on a slider `width` wide.
fn dragged_value(drag: &ParamDrag, x: f32, width: f32, range: &RangeInclusive<f32>) -> f32 {
    let speed = if drag.fine { FINE_DRAG } else { 1. };
    let per_px = (range.end() - range.start()) / width * speed;
    (drag.anchor_value + (x - drag.anchor_x) * per_px).clamp(*range.start(), *range.end())
}

/// A volume or pan slider's area.
fn slider_rect(ui: &mut Ui, width: f32) -> (Rect, Response) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, SLIDER_HEIGHT), Sense::click_and_drag());
    (
        rect,
        response.on_hover_and_drag_cursor(egui::CursorIcon::ResizeHorizontal),
    )
}

/// Well, `fill` in the track colour, and the value on top.
fn paint_slider(ui: &Ui, rect: Rect, fill: Rect, color: Color32, text: &str) {
    let theme = ui.app_theme();
    let painter = ui.painter();
    painter.rect_filled(rect, 1.0, theme.bg_deep);
    painter.rect_filled(fill, 1.0, color);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::new(font_size::SMALL, egui::FontFamily::Proportional),
        theme.text,
    );
}

fn db_to_volume(gain: f32) -> f32 {
    10f32.powf(gain / 20.)
}

/// `-3.2`, or `-inf` for silence.
fn format_gain(gain: f32) -> String {
    if gain.is_finite() {
        format!("{gain:.1}")
    } else {
        "-inf".into()
    }
}

/// `C` for centre, else `L`/`R` and a percentage: `L35`.
fn format_pan(pan: f32) -> String {
    let percent = (pan.abs() * 100.).round();
    match percent as u32 {
        0 => "C".into(),
        n if pan < 0. => format!("L{n}"),
        n => format!("R{n}"),
    }
}

/// Close enough to the centre to be meant as the centre.
fn snap_to_centre(pan: f32) -> f32 {
    if pan.abs() < PAN_SNAP { 0. } else { pan }
}

/// Track header toggle: `fill` when on, the default control colour when off.
fn toggle_button(text: &str, fill: Option<Color32>, theme: &Theme) -> FlatButton {
    let button = FlatButton::new(text).square(BUTTON_SIZE);
    match fill {
        Some(fill) => button.fill(fill).color(theme.text_on(fill)),
        None => button,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 45 dB over 90 px: 0.5 dB per pixel.
    const WIDTH: f32 = 90.;

    fn drag(fine: bool) -> ParamDrag {
        ParamDrag {
            start: 1.,
            anchor_x: 100.,
            anchor_value: 0.,
            fine,
        }
    }

    #[test]
    fn value_follows_the_pointer_past_the_ends() {
        let drag = drag(false);
        assert_eq!(dragged_value(&drag, 104., WIDTH, &GAIN_RANGE), 2.);
        // Past the top it stays at the top...
        assert_eq!(dragged_value(&drag, 150., WIDTH, &GAIN_RANGE), 5.);
        // ...and coming back gives the same value as on the way out.
        assert_eq!(dragged_value(&drag, 104., WIDTH, &GAIN_RANGE), 2.);
        assert_eq!(dragged_value(&drag, -500., WIDTH, &GAIN_RANGE), -40.);
        assert_eq!(dragged_value(&drag, 100., WIDTH, &GAIN_RANGE), 0.);
    }

    #[test]
    fn shift_drags_ten_times_slower() {
        assert_eq!(dragged_value(&drag(true), 120., WIDTH, &GAIN_RANGE), 1.);
    }

    #[test]
    fn pan_formats_and_snaps_to_centre() {
        assert_eq!(format_pan(0.), "C");
        assert_eq!(format_pan(0.004), "C");
        assert_eq!(format_pan(-0.35), "L35");
        assert_eq!(format_pan(1.), "R100");
        assert_eq!(snap_to_centre(0.01), 0.);
        assert_eq!(snap_to_centre(-0.2), -0.2);
        // Pan drags over -1..1 like any slider.
        assert_eq!(dragged_value(&drag(false), 145., WIDTH, &PAN_RANGE), 1.);
    }

    #[test]
    fn gains_format_with_silence() {
        assert_eq!(format_gain(-3.25), "-3.2");
        assert_eq!(format_gain(20. * 0f32.log10()), "-inf");
        assert_eq!(db_to_volume(0.), 1.);
    }
}
