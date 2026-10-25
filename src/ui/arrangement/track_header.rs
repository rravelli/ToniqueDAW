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
    Align, Color32, FontId, Frame, Label, Layout, Margin, Rect, Response, RichText, Sense, Stroke,
    TextEdit, Ui, UiBuilder, Vec2, Widget, pos2, vec2,
};
use egui_phosphor::{
    fill::{COPY, FOLDER_MINUS, FOLDER_PLUS, PALETTE, PLUS, TRASH},
    regular::{MUSIC_NOTE_SIMPLE, TEXT_T},
};
use std::ops::RangeInclusive;

/// Space between a header's edges and its content.
const PADDING: f32 = 4.;
/// Toggles (mute, solo, arm) and the caret.
const BUTTON_SIZE: Vec2 = vec2(18., 16.);
const CARET_WIDTH: f32 = 14.;
const METER_WIDTH: f32 = 12.;
/// Between controls on a line.
const GAP: f32 = 3.;
/// Between the lines of controls.
const LINE_GAP: f32 = 4.;
const SLIDER_HEIGHT: f32 = 16.;
const PAN_WIDTH: f32 = 34.;
/// Wide enough for the volume to show its unit.
const VOLUME_UNIT_WIDTH: f32 = 64.;
pub const ROW_GAP: f32 = 3.0;
/// Indentation per level of groups, up to [`MAX_INDENT_LEVELS`] (deeper
/// rows line up with the deepest shown level).
const INDENT: f32 = 8.;
const MAX_INDENT_LEVELS: usize = 6;
/// Width of the coloured bar on the left of each header.
pub const COLOR_BAR_WIDTH: f32 = 4.;
/// Volume slider range, in dB: its bottom is silence.
pub const VOLUME_RANGE: RangeInclusive<f32> = -40.0..=5.0;

/// Left of the coloured bar of a row at `depth`, in a header starting at
/// `left`. Groups extend theirs down to their last row from there.
pub fn color_bar_x(left: f32, depth: usize) -> f32 {
    left + PADDING + depth.min(MAX_INDENT_LEVELS) as f32 * INDENT
}

/// Space between a header's edge and its content, top and bottom.
pub const HEADER_INSET: f32 = PADDING;

/// Where everything in a header goes, from its rect: the same for tracks,
/// groups, the master and ghost headers, at any height. The first line
/// (caret, name, toggles) is at the top, or centred in a collapsed row;
/// the volume and pan go below it when there's room; the meter runs down
/// the right edge.
struct HeaderLayout {
    bar: Rect,
    caret: Rect,
    name: Rect,
    /// Mute, solo, then arm when shown.
    toggles: Vec<Rect>,
    /// Volume and pan.
    sliders: Option<(Rect, Rect)>,
    meter: Rect,
}

impl HeaderLayout {
    fn new(rect: Rect, depth: usize, collapsed: bool, toggles: usize) -> Self {
        let inner = rect.shrink(PADDING);
        let bar_left = color_bar_x(rect.left(), depth);
        let bar = Rect::from_x_y_ranges(bar_left..=bar_left + COLOR_BAR_WIDTH, inner.y_range());
        let meter =
            Rect::from_x_y_ranges(inner.right() - METER_WIDTH..=inner.right(), inner.y_range());
        let content =
            Rect::from_x_y_ranges(bar.right() + GAP..=meter.left() - GAP, inner.y_range());
        let has_sliders =
            !collapsed && content.height() >= BUTTON_SIZE.y + LINE_GAP + SLIDER_HEIGHT;
        let line_top = if has_sliders {
            content.top()
        } else {
            content.center().y - BUTTON_SIZE.y / 2.
        };
        let line = Rect::from_x_y_ranges(content.x_range(), line_top..=line_top + BUTTON_SIZE.y);
        let caret = Rect::from_min_size(line.min, vec2(CARET_WIDTH, BUTTON_SIZE.y));
        let mut right = line.right();
        let mut toggle_rects: Vec<Rect> = (0..toggles)
            .map(|_| {
                let toggle =
                    Rect::from_min_size(pos2(right - BUTTON_SIZE.x, line.top()), BUTTON_SIZE);
                right = toggle.left() - 1.;
                toggle
            })
            .collect();
        toggle_rects.reverse();
        let name = Rect::from_x_y_ranges(caret.right() + 2.0..=right - GAP, line.y_range());
        let sliders = has_sliders.then(|| {
            let top = line.bottom() + LINE_GAP;
            let y = top..=top + SLIDER_HEIGHT;
            let pan =
                Rect::from_x_y_ranges(content.right() - PAN_WIDTH..=content.right(), y.clone());
            let volume = Rect::from_x_y_ranges(caret.left()..=pan.left() - GAP, y);
            (volume, pan)
        });
        Self {
            bar,
            caret,
            name,
            toggles: toggle_rects,
            sliders,
            meter,
        }
    }
}

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
        let is_master = track.id == MASTER_TRACK_ID;
        // Groups and the master have no arm button; the master no solo.
        let armable = RECORDING && !is_group && !is_master;
        let toggle_count = if is_master { 1 } else { 2 + armable as usize };

        let theme = ui.app_theme();
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), track.height), Sense::hover());
        let layout = HeaderLayout::new(rect, track.depth, track.collapsed, toggle_count);
        let fill = if track.selected {
            theme.bg_control
        } else {
            theme.bg_raised
        };
        ui.painter().rect_filled(rect, 2., fill);
        let bar = if silenced {
            theme.text_disabled
        } else {
            track.color
        };
        ui.painter().rect_filled(layout.bar, 1., bar);

        // The header itself, under its controls: select, drag, double-click.
        let response = ui.interact(
            rect,
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

        if put(ui, layout.caret, collapse_button(track.collapsed)).clicked() {
            state.set_collapsed(&track.id, !track.collapsed);
        }
        in_rect(ui, layout.name, |ui| self.text_ui(ui, track, state));
        let mut toggles = layout.toggles.iter();
        if let Some(&r) = toggles.next()
            && put(
                ui,
                r,
                toggle_button("M", track.muted.then_some(theme.warning), &theme),
            )
            .clicked()
        {
            state.set_mute(track.id, !track.muted);
        }
        if let Some(&r) = toggles.next()
            && put(ui, r, solo_button(is_solo, track, &theme)).clicked()
        {
            state.toggle_solo(track.id, ui.input(|i| i.modifiers.shift));
        }
        if let Some(&r) = toggles.next()
            && put(
                ui,
                r,
                toggle_button(
                    MUSIC_NOTE_SIMPLE,
                    track.armed.then_some(theme.record),
                    &theme,
                ),
            )
            .clicked()
        {
            state.set_armed(&track.id, !track.armed);
        }
        if let Some((volume, pan)) = layout.sliders {
            in_rect(ui, volume, |ui| {
                volume_slider(ui, track, state, volume.size())
            });
            in_rect(ui, pan, |ui| pan_slider(ui, track, state, pan.size()));
        }
        if let Some(metrics) = state.metrics.tracks.get(&track.id) {
            let meter = LevelMeter::new(layout.meter.size(), metrics).disabled(silenced);
            if put(ui, layout.meter, meter).clicked() {
                state.reset_meter_clip(track.id);
            }
        }
        response.context_menu(|ui| {
            self.context_menu(ui, track, state);
        });
        // Drag area
        self.resize_handle(ui, track, state);

        // Save temporary state
        ui.data_mut(|w| w.insert_temp(id, self.clone()));
        response
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
                // The master can't be renamed, moved, grouped, copied or
                // deleted: only its colour changes.
                if track.id == MASTER_TRACK_ID {
                    color_menu(ui, track, state);
                    return;
                }
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
                color_menu(ui, track, state);
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

/// Volume in dB, filled from the left; with its unit when wide enough.
fn volume_slider(ui: &mut Ui, track: &TrackRow, state: &mut ProjectState, size: Vec2) {
    let mut slider = ParamSlider::gain(track.volume, VOLUME_RANGE)
        .color(track.color)
        .size(size);
    if size.x >= VOLUME_UNIT_WIDTH {
        slider = slider.suffix(" dB");
    }
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
fn pan_slider(ui: &mut Ui, track: &TrackRow, state: &mut ProjectState, size: Vec2) {
    let slider = ParamSlider::pan(track.pan).color(track.color).size(size);
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
/// track a drop would add. Laid out as [`TrackHeader`], so the real one
/// takes its place without a jump.
pub fn ghost_header(ui: &mut Ui, track: &TrackRow) {
    let theme = ui.app_theme();
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), track.height), Sense::hover());
    // Opaque under the fading, as the real one is.
    ui.painter().rect_filled(rect, 2., theme.bg_raised);
    ui.disable();
    let layout = HeaderLayout::new(rect, track.depth, false, 2 + RECORDING as usize);
    ui.painter().rect_filled(layout.bar, 1., track.color);
    put(ui, layout.caret, collapse_button(false));
    in_rect(ui, layout.name, |ui| name_label(ui, track));
    for (rect, text) in layout.toggles.iter().zip(["M", "S", MUSIC_NOTE_SIMPLE]) {
        put(ui, *rect, toggle_button(text, None, &theme));
    }
    if let Some((volume, pan)) = layout.sliders {
        in_rect(ui, volume, |ui| {
            ParamSlider::gain(track.volume, VOLUME_RANGE)
                .color(track.color)
                .size(volume.size())
                .show(ui)
        });
        in_rect(ui, pan, |ui| {
            ParamSlider::pan(track.pan)
                .color(track.color)
                .size(pan.size())
                .show(ui)
        });
    }
}

/// `widget` exactly over `rect`.
fn put(ui: &mut Ui, rect: Rect, widget: impl Widget) -> Response {
    in_rect(ui, rect, |ui| ui.add_sized(rect.size(), widget))
}

/// `add_contents` laid out left to right in `rect`, centred vertically.
/// The header's rect is already taken: this takes no more space.
fn in_rect<R>(ui: &mut Ui, rect: Rect, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    add_contents(
        &mut ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center)),
        ),
    )
}

/// The track's name, as shown when not renaming it.
fn name_label(ui: &mut Ui, track: &TrackRow) {
    ui.add(
        Label::new(
            RichText::new(display_name(&track.name, track.first_track_index))
                .color(ui.app_theme().text)
                .size(font_size::LABEL),
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
    FlatButton::ghost(icon)
        .font(FontId::new(
            font_size::SMALL,
            egui::FontFamily::Name(PHOSPHOR_FILL.into()),
        ))
        .size(vec2(CARET_WIDTH, BUTTON_SIZE.y))
}

/// "Color", opening a palette for the row's colour.
fn color_menu(ui: &mut Ui, track: &TrackRow, state: &mut ProjectState) {
    ContextMenuButton::new(PALETTE, "Color").submenu(ui, |ui| {
        let mut color = track.color;
        let picker = ColorSelect::new(("track-color", track.id), &mut color);
        if ui.add(picker).changed() {
            state.track_view_mut(&track.id).color = color;
            state.commit_track_view(&track.id);
        }
    });
}

/// In the track's colour when soloed; dimmed when the track still isn't
/// heard (muted, or in a muted group: mute wins over solo).
fn solo_button(solo: bool, track: &TrackRow, theme: &Theme) -> FlatButton {
    let fill = match (solo, track.is_silenced()) {
        (false, _) => None,
        (true, false) => Some(track.color),
        (true, true) => Some(with_alpha(track.color, 90)),
    };
    toggle_button("S", fill, theme)
}

/// Track header toggle: `fill` when on, outlined when off.
fn toggle_button(text: &str, fill: Option<Color32>, theme: &Theme) -> FlatButton {
    let button = FlatButton::new(text)
        .size(BUTTON_SIZE)
        .font(FontId::proportional(font_size::SMALL))
        .border_radius(2.);
    match fill {
        Some(fill) => button.fill(fill).color(theme.text_on(fill)),
        None => button
            .fill(Color32::TRANSPARENT)
            .color(theme.text_muted)
            .stroke(Stroke::new(1., theme.border)),
    }
}
