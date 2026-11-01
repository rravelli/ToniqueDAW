//! The clip's properties, left of its editor.

use crate::ui::font::semibold;
use egui::{
    Align2, Color32, DragValue, FontId, Label, Rect, RichText, Sense, Stroke, Ui, pos2, vec2,
};
use tonique_engine::{
    edit::ClipId,
    time::{BeatPos, TimeSignature},
};

use crate::{
    core::{
        clip::{AudioClip, MIN_LENGTH},
        state::{ClipParam, ProjectState},
    },
    ui::{
        theme::{ThemeExt, font_size},
        widget::{
            flat_button::FlatButton,
            param_slider::{ParamSlider, SliderEdit},
            tab_bar::TabBar,
        },
    },
    utils::{format_bars, format_time, parse_bars, parse_parts},
};

use super::{
    markers::{TrimEdge, default_loop, trimmed, with_loop},
    warp::warp_ui,
};

/// Between the inspector's cells, side by side.
pub(super) const CELL_GAP: f32 = 4.;
/// Above each section of the inspector.
const SECTION_GAP: f32 = 4.;
/// The loop's on/off switch, on its section's title row.
const LOOP_SWITCH_WIDTH: f32 = 28.;
/// The inspector's values.
pub(super) const FIELD_HEIGHT: f32 = 18.;

/// The clip's properties, in sections as in other DAWs' inspectors: where
/// it is (start, end, length) and what it plays (offset in the file, gain,
/// pan). Positions can be dragged, or clicked and typed; edits show while
/// dragged and apply as one undo step once released (or typed). Gain and
/// pan are sliders like a track's, heard as they're dragged.
pub(super) fn inspector_ui(
    ui: &mut Ui,
    clip: &AudioClip,
    color: Color32,
    state: &mut ProjectState,
) {
    let bpm = state.bpm();
    let bpb = state.time_signature();
    let shown = state.shown_clip(clip);
    let start = shown.position.0;
    let end = shown.end(bpm).0;
    let length = end - start;
    let source = shown.source_seconds() as f64;
    let offset = shown.offset;
    let free = ui.input(|i| i.modifiers.alt);
    // Positions and lengths snap to sixteenths, unless Alt is held.
    let snap = |beats: f64| {
        if free {
            beats
        } else {
            (beats * 4.).round() / 4.
        }
    };
    // The clip as edited, and whether the edit is done: shown as edited
    // until then, or (`live`) heard as it's edited, one undo step either way.
    let mut change: Option<(AudioClip, bool)> = None;
    let mut live: Option<(AudioClip, bool)> = None;

    ui.style_mut().override_font_id = Some(FontId::proportional(font_size::LABEL));
    ui.spacing_mut().item_spacing = vec2(CELL_GAP, 1.);

    // What's edited, in tabs: the last picked, whichever clip.
    let tab_id = egui::Id::new("clip-inspector-tab");
    let mut tab = ui
        .data(|d| d.get_temp(tab_id))
        .unwrap_or(InspectorTab::Clip);
    ui.add(TabBar::new(
        &mut tab,
        [
            (InspectorTab::Clip, "Clip"),
            (InspectorTab::Audio, "Audio"),
            (InspectorTab::Warp, "Warp"),
            (InspectorTab::Loop, "Loop"),
        ],
    ));
    ui.data_mut(|d| d.insert_temp(tab_id, tab));
    ui.add_space(SECTION_GAP);

    if tab == InspectorTab::Clip {
        ui.columns(3, |cells| {
            let raw = cells[0].id().with("start-raw");
            let mut beats = cells[0].data(|d| d.get_temp(raw)).unwrap_or(start);
            let response = cell(
                &mut cells[0],
                "Start",
                DragValue::new(&mut beats)
                    .speed(1. / 16.)
                    .range(0.0..=f64::MAX)
                    .custom_formatter(|v, _| format_bars(v, bpb))
                    .custom_parser(|text| parse_bars(text, bpb)),
            );
            keep_raw(&cells[0], raw, &response, beats);
            if let Some(done) = gesture(&response) {
                let mut moved = shown.clone();
                moved.position = BeatPos(snap(beats).max(0.));
                change = Some((moved, done));
            }

            let raw = cells[1].id().with("end-raw");
            let mut beats = cells[1].data(|d| d.get_temp(raw)).unwrap_or(end);
            let response = cell(
                &mut cells[1],
                "End",
                DragValue::new(&mut beats)
                    .speed(1. / 16.)
                    .range(start + MIN_LENGTH..=f64::MAX)
                    .custom_formatter(|v, _| format_bars(v, bpb))
                    .custom_parser(|text| parse_bars(text, bpb)),
            );
            keep_raw(&cells[1], raw, &response, beats);
            if let Some(done) = gesture(&response) {
                let at = BeatPos(snap(beats).max(start + MIN_LENGTH));
                change = Some((trimmed(&shown, TrimEdge::End, at, bpm), done));
            }

            let raw = cells[2].id().with("length-raw");
            let mut beats = cells[2].data(|d| d.get_temp(raw)).unwrap_or(length);
            let response = cell(
                &mut cells[2],
                "Length",
                DragValue::new(&mut beats)
                    .speed(1. / 16.)
                    .range(MIN_LENGTH..=f64::MAX)
                    .custom_formatter(|v, _| format_length(v, bpb))
                    .custom_parser(|text| parse_length(text, bpb)),
            );
            keep_raw(&cells[2], raw, &response, beats);
            if let Some(done) = gesture(&response) {
                let at = shown.position + snap(beats).max(MIN_LENGTH);
                change = Some((trimmed(&shown, TrimEdge::End, at, bpm), done));
            }
        });
    }
    if tab == InspectorTab::Audio {
        ui.columns(3, |cells| {
            let mut seconds = offset;
            let response = cell(
                &mut cells[0],
                "Offset",
                DragValue::new(&mut seconds)
                    .speed(0.005)
                    .range(0.0..=f64::MAX)
                    .custom_formatter(|v, _| format_time(v))
                    .custom_parser(parse_time),
            );
            if let Some(done) = gesture(&response) {
                let mut slipped = shown.clone();
                slipped.slip_to(seconds, bpm);
                change = Some((slipped, done));
            }

            // As on tracks: a level in the clip's colour, reset by a
            // double-click; heard as it moves, one undo step once released.
            cell_label(&mut cells[1], "Gain");
            let size = vec2(cells[1].available_width(), FIELD_HEIGHT);
            let gain = ParamSlider::gain(shown.gain, CLIP_GAIN_RANGE)
                .field()
                .color(color)
                .size(size)
                .suffix(" dB");
            apply_param(state, clip.id, ClipParam::Gain, gain.show(&mut cells[1]).1);

            cell_label(&mut cells[2], "Pan");
            let (_, edit) = ParamSlider::pan(shown.pan)
                .field()
                .color(color)
                .size(size)
                .show(&mut cells[2]);
            apply_param(state, clip.id, ClipParam::Pan, edit);
        });
    }
    if tab == InspectorTab::Warp {
        warp_ui(ui, &shown, bpm, &mut change, &mut live);
    }
    // Its switch on the title's row.
    if tab == InspectorTab::Loop {
        let looped = shown.active_loop();
        let header = section(ui, "Loop", LOOP_SWITCH_WIDTH + CELL_GAP);
        let switch = Rect::from_min_max(
            pos2(header.right() - LOOP_SWITCH_WIDTH, header.top()),
            header.max,
        );
        let toggle = FlatButton::new(if looped.is_some() { "On" } else { "Off" })
            .size(switch.size())
            .font(FontId::proportional(font_size::SMALL))
            .selected(looped.is_some())
            .tooltip("Play the loop over and over, for as long as the clip lasts");
        if ui.put(switch, toggle).clicked() {
            let mut toggled = shown.clone();
            match looped {
                Some(_) => toggled.set_loop(None, bpm),
                None => toggled.set_loop(Some(default_loop(&shown, bpm)), bpm),
            }
            change = Some((toggled, true));
        }
        if let Some(l) = looped {
            ui.columns(3, |cells| {
                let mut start = l.start_s;
                let response = cell(
                    &mut cells[0],
                    "Start",
                    DragValue::new(&mut start)
                        .speed(0.005)
                        .range(0.0..=source)
                        .custom_formatter(|v, _| format_time(v))
                        .custom_parser(parse_time),
                );
                if let Some(done) = gesture(&response) {
                    change = Some((with_loop(&shown, start, l.end_s, bpm), done));
                }

                let raw = cells[1].id().with("loop-length-raw");
                let loop_length = shown.file_beats(l.end_s - l.start_s, bpm);
                let mut beats = cells[1].data(|d| d.get_temp(raw)).unwrap_or(loop_length);
                let response = cell(
                    &mut cells[1],
                    "Length",
                    DragValue::new(&mut beats)
                        .speed(1. / 16.)
                        .range(MIN_LENGTH..=f64::MAX)
                        .custom_formatter(|v, _| format_length(v, bpb))
                        .custom_parser(|text| parse_length(text, bpb)),
                );
                keep_raw(&cells[1], raw, &response, beats);
                if let Some(done) = gesture(&response) {
                    let end = l.start_s + shown.file_seconds(snap(beats).max(MIN_LENGTH), bpm);
                    change = Some((with_loop(&shown, l.start_s, end, bpm), done));
                }
            });
        }
    }

    if let Some((edited, done)) = live {
        state.edit_clip_live(edited, done);
    }
    match change {
        Some((edited, false)) => state.resize_clip(edited),
        Some((edited, true)) => state.commit_resize_clip(edited),
        None => {}
    }
}

/// The inspector's tabs.
#[derive(Clone, Copy, PartialEq)]
enum InspectorTab {
    /// Where it is: start, end, length.
    Clip,
    /// What it plays: offset, gain, pan.
    Audio,
    /// How it follows the tempo, and its transposition.
    Warp,
    Loop,
}

/// A section's title, small and muted, with a rule after it up to
/// `reserve` from the right (for something on its row). Returns its row.
pub(super) fn section(ui: &mut Ui, title: &str, reserve: f32) -> Rect {
    ui.add_space(SECTION_GAP);
    let theme = ui.app_theme();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.), Sense::hover());
    let painter = ui.painter();
    let text = painter.text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        title.to_uppercase(),
        semibold(font_size::SMALL),
        theme.text_muted,
    );
    let rule_end = rect.right() - reserve;
    if text.right() + 6. < rule_end {
        painter.hline(
            text.right() + 6.0..=rule_end,
            rect.center().y,
            Stroke::new(1., theme.separator),
        );
    }
    ui.add_space(2.);
    rect
}

/// A cell's label, small and muted, above its value.
pub(super) fn cell_label(ui: &mut Ui, text: &str) {
    ui.add(
        Label::new(
            RichText::new(text)
                .size(font_size::SMALL)
                .color(ui.app_theme().text_muted),
        )
        .truncate(),
    );
}

/// A cell: its label above a value field filling its width.
pub(super) fn cell(ui: &mut Ui, label: &str, value: DragValue) -> egui::Response {
    cell_label(ui, label);
    let width = ui.available_width();
    ui.add_sized(vec2(width, FIELD_HEIGHT), value)
}

/// Apply a gain or pan slider's edit to the clip's parameter.
pub(super) fn apply_param(
    state: &mut ProjectState,
    clip: ClipId,
    which: ClipParam,
    edit: SliderEdit,
) {
    match edit {
        SliderEdit::Moved(value) => state.set_clip_param(clip, which, value),
        SliderEdit::Released { start, value } => state.commit_clip_param(clip, which, start, value),
        SliderEdit::Reset { from, to } => state.commit_clip_param(clip, which, from, to),
        SliderEdit::None => {}
    }
}

/// Keep a dragged value as it is, before snapping, so that snapping what's
/// shown doesn't hold back small drags.
pub(super) fn keep_raw(ui: &Ui, id: egui::Id, response: &egui::Response, value: f64) {
    if response.dragged() {
        ui.data_mut(|d| d.insert_temp(id, value));
    } else {
        ui.data_mut(|d| d.remove::<f64>(id));
    }
}

/// Where a drag value's edit is at: `Some(false)` while dragged,
/// `Some(true)` once released or typed, `None` when untouched.
pub(super) fn gesture(response: &egui::Response) -> Option<bool> {
    if response.drag_stopped() {
        Some(true)
    } else if response.changed() {
        Some(!response.dragged())
    } else {
        None
    }
}

/// The gain slider's range, in dB: its bottom is silence, its top the
/// engine's limit (+12 dB).
pub(super) const CLIP_GAIN_RANGE: std::ops::RangeInclusive<f32> = -40.0..=12.0;

/// A length as typed, `bars.beats.sixteenths` (`1.2.0`; missing parts are
/// none), in beats.
pub(super) fn parse_length(text: &str, signature: TimeSignature) -> Option<f64> {
    let parts = parse_parts(text)?;
    let part = |i: usize| parts.get(i).copied().unwrap_or(0.);
    if parts.is_empty() || parts.iter().any(|p| *p < 0.) {
        return None;
    }
    Some(part(0) * signature.beats_per_bar() + part(1) * signature.beat_length() + part(2) / 4.)
}

/// A length as `bars.beats.sixteenths`, counting from 0, beats as
/// `signature` counts them: one bar and two beats is `1.2.0`.
pub(super) fn format_length(beats: f64, signature: TimeSignature) -> String {
    let sixteenths = (beats * 4.).round().max(0.) as u64;
    // Whole numbers of sixteenths, whatever the signature.
    let per_bar = ((signature.beats_per_bar() * 4.).round() as u64).max(1);
    let per_beat = ((signature.beat_length() * 4.).round() as u64).max(1);
    let within = sixteenths % per_bar;
    format!(
        "{}.{}.{}",
        sixteenths / per_bar,
        within / per_beat,
        within % per_beat
    )
}

/// A time as typed: `1:23.250`, `83.25` (seconds) or `0:05`.
pub(super) fn parse_time(text: &str) -> Option<f64> {
    let text = text.trim();
    match text.split_once(':') {
        Some((minutes, seconds)) => {
            let minutes: f64 = minutes.trim().parse().ok()?;
            let seconds: f64 = seconds.trim().parse().ok()?;
            (minutes >= 0. && seconds >= 0.).then_some(minutes * 60. + seconds)
        }
        None => text.parse().ok().filter(|s: &f64| *s >= 0.),
    }
}
