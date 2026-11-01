//! The clip's properties, left of its editor.

use crate::ui::font::semibold;
use egui::{
    Align2, Color32, DragValue, FontId, Label, Rect, RichText, ScrollArea, Sense, Stroke, Ui,
    UiBuilder, pos2, vec2,
};
use egui_phosphor::regular as icons;
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
        widget::{flat_button::FlatButton, param_slider::SliderEdit},
    },
    utils::{format_bars, format_time, parse_bars, parse_parts},
};

use super::{
    audio::audio_panel,
    markers::{TrimEdge, default_loop, trimmed, with_loop},
};

/// Between the inspector's cells, side by side.
pub(super) const CELL_GAP: f32 = 4.;
/// Above each section of the inspector.
const SECTION_GAP: f32 = 4.;
const SECTION_TITLE_HEIGHT: f32 = 14.;
/// Either side of a section's title, as a toggle.
const TOGGLE_PADDING: f32 = 5.;
/// The inspector's values.
pub(super) const FIELD_HEIGHT: f32 = 18.;
/// The column of panel toggles, left of the panels.
pub(super) const RAIL_WIDTH: f32 = 24.;
const PANEL_WIDTH: f32 = 196.;
/// Between the rail and the panels, and between panels: a rule in its
/// middle.
const PANEL_GAP: f32 = 9.;

/// The panels shown, toggled on the inspector's rail: the last picked,
/// whichever clip.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct Panels {
    /// Where it is: start, end, length, offset, and its loop.
    clip: bool,
    /// How it sounds: gain, pan, warp and transposition.
    audio: bool,
}

impl Default for Panels {
    fn default() -> Self {
        Self {
            clip: true,
            audio: true,
        }
    }
}

impl Panels {
    fn id() -> egui::Id {
        egui::Id::new("clip-inspector-panels")
    }

    pub(super) fn get(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp(Self::id())).unwrap_or_default()
    }

    fn set(self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(Self::id(), self));
    }

    /// The inspector's width: its rail and the panels shown.
    pub(super) fn width(self) -> f32 {
        let shown = self.clip as u8 + self.audio as u8;
        RAIL_WIDTH + shown as f32 * (PANEL_GAP + PANEL_WIDTH)
    }
}

#[derive(Clone, Copy, Debug, Hash)]
enum Panel {
    Clip,
    Audio,
}

/// The clip's properties in panels side by side, each shown or hidden from
/// the rail on their left. Positions can be dragged, or clicked and typed;
/// edits show while dragged and apply as one undo step once released (or
/// typed). Gain, pan and the transposition are heard as they're dragged.
pub(super) fn inspector_ui(
    ui: &mut Ui,
    clip: &AudioClip,
    color: Color32,
    state: &mut ProjectState,
) {
    let bpm = state.bpm();
    let bpb = state.time_signature();
    let shown = state.shown_clip(clip);
    // The clip as edited, and whether the edit is done: shown as edited
    // until then, or (`live`) heard as it's edited, one undo step either way.
    let mut change: Option<(AudioClip, bool)> = None;
    let mut live: Option<(AudioClip, bool)> = None;

    let rect = ui.max_rect();
    ui.set_clip_rect(rect.intersect(ui.clip_rect()));
    let theme = ui.app_theme();
    let mut panels = Panels::get(ui.ctx());
    let mut top = rect.top();
    for (on, icon, name) in [
        (&mut panels.clip, icons::BROWSER, "Clip"),
        (&mut panels.audio, icons::WAVEFORM, "Audio"),
    ] {
        let button = FlatButton::ghost(icon)
            .square(RAIL_WIDTH)
            .font(FontId::proportional(font_size::HEADING))
            .selected(*on)
            .tooltip(name);
        let at = Rect::from_min_size(pos2(rect.left(), top), vec2(RAIL_WIDTH, RAIL_WIDTH));
        if ui.put(at, button).clicked() {
            *on = !*on;
        }
        top += RAIL_WIDTH + 2.;
    }
    panels.set(ui.ctx());

    // Too narrow for them all, the panels scroll sideways under the rail.
    let shown_panels: Vec<Panel> = [(panels.clip, Panel::Clip), (panels.audio, Panel::Audio)]
        .into_iter()
        .filter_map(|(on, panel)| on.then_some(panel))
        .collect();
    let area = rect.with_min_x(rect.left() + RAIL_WIDTH);
    ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
        // Shown whenever they're cut, not only on hover.
        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        ScrollArea::horizontal()
            .id_salt("clip-inspector-panels")
            .auto_shrink(false)
            .show(ui, |ui| {
                let width = shown_panels.len() as f32 * (PANEL_GAP + PANEL_WIDTH);
                let height = ui.available_height();
                let (content, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
                let mut left = content.left();
                for &panel in &shown_panels {
                    ui.painter().vline(
                        left + PANEL_GAP / 2.,
                        content.y_range(),
                        Stroke::new(1., theme.separator),
                    );
                    let at = Rect::from_min_size(
                        pos2(left + PANEL_GAP, content.top()),
                        vec2(PANEL_WIDTH, height),
                    );
                    left = at.right();
                    ui.scope_builder(UiBuilder::new().max_rect(at), |ui| {
                        ui.style_mut().override_font_id =
                            Some(FontId::proportional(font_size::LABEL));
                        ui.spacing_mut().item_spacing = vec2(CELL_GAP, 1.);
                        ScrollArea::vertical()
                            .id_salt(("clip-inspector", panel))
                            .auto_shrink(false)
                            .show(ui, |ui| match panel {
                                Panel::Clip => clip_panel(ui, &shown, bpm, bpb, &mut change),
                                Panel::Audio => audio_panel(
                                    ui,
                                    &shown,
                                    color,
                                    bpm,
                                    state,
                                    &mut change,
                                    &mut live,
                                ),
                            });
                    });
                }
            });
    });

    if let Some((edited, done)) = live {
        state.edit_clip_live(edited, done);
    }
    match change {
        Some((edited, false)) => state.resize_clip(edited),
        Some((edited, true)) => state.commit_resize_clip(edited),
        None => {}
    }
}

/// Where the clip is (start, end, length), where in its file it starts
/// (offset), and its loop, switched on its title's row.
fn clip_panel(
    ui: &mut Ui,
    shown: &AudioClip,
    bpm: f32,
    bpb: TimeSignature,
    change: &mut Option<(AudioClip, bool)>,
) {
    let start = shown.position.0;
    let end = shown.end(bpm).0;
    let length = end - start;
    let source = shown.source_seconds() as f64;
    let free = ui.input(|i| i.modifiers.alt);
    // Positions and lengths snap to sixteenths, unless Alt is held.
    let snap = |beats: f64| {
        if free {
            beats
        } else {
            (beats * 4.).round() / 4.
        }
    };

    section(ui, "Clip", 0.);
    ui.columns(2, |cells| {
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
            *change = Some((moved, done));
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
            *change = Some((trimmed(shown, TrimEdge::End, at, bpm), done));
        }
    });
    ui.add_space(CELL_GAP);
    ui.columns(2, |cells| {
        let raw = cells[0].id().with("length-raw");
        let mut beats = cells[0].data(|d| d.get_temp(raw)).unwrap_or(length);
        let response = cell(
            &mut cells[0],
            "Length",
            DragValue::new(&mut beats)
                .speed(1. / 16.)
                .range(MIN_LENGTH..=f64::MAX)
                .custom_formatter(|v, _| format_length(v, bpb))
                .custom_parser(|text| parse_length(text, bpb)),
        );
        keep_raw(&cells[0], raw, &response, beats);
        if let Some(done) = gesture(&response) {
            let at = shown.position + snap(beats).max(MIN_LENGTH);
            *change = Some((trimmed(shown, TrimEdge::End, at, bpm), done));
        }

        let mut seconds = shown.offset;
        let response = cell(
            &mut cells[1],
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
            *change = Some((slipped, done));
        }
    });

    // Off, it shows where it would be, greyed out.
    let looped = shown.active_loop();
    if toggle_section(ui, "Loop", looped.is_some(), 0.).1 {
        let mut toggled = shown.clone();
        match looped {
            Some(_) => toggled.set_loop(None, bpm),
            None => toggled.set_loop(Some(default_loop(shown, bpm)), bpm),
        }
        *change = Some((toggled, true));
    }
    let l = looped.unwrap_or_else(|| default_loop(shown, bpm));
    ui.add_enabled_ui(looped.is_some(), |ui| {
        ui.columns(2, |cells| {
            let mut start = l.start_s;
            let response = cell(
                &mut cells[0],
                "Position",
                DragValue::new(&mut start)
                    .speed(0.005)
                    .range(0.0..=source)
                    .custom_formatter(|v, _| format_time(v))
                    .custom_parser(parse_time),
            );
            if let Some(done) = gesture(&response) {
                *change = Some((with_loop(shown, start, l.end_s, bpm), done));
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
                *change = Some((with_loop(shown, l.start_s, end, bpm), done));
            }
        });
    });
}

/// A section switched on and off by its title, a toggle as tall as a
/// field, with a rule after it up to `reserve` from the right (for
/// something on its row). Returns its row and whether the toggle was
/// clicked.
pub(super) fn toggle_section(ui: &mut Ui, title: &str, on: bool, reserve: f32) -> (Rect, bool) {
    ui.add_space(SECTION_GAP);
    let theme = ui.app_theme();
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), FIELD_HEIGHT), Sense::hover());
    let title = title.to_uppercase();
    let font = semibold(font_size::SMALL);
    let width = ui
        .painter()
        .layout_no_wrap(title.clone(), font.clone(), theme.text)
        .size()
        .x
        + 2. * TOGGLE_PADDING;
    let at = Rect::from_min_size(rect.min, vec2(width, FIELD_HEIGHT));
    let toggle = FlatButton::new(title)
        .size(at.size())
        .font(font)
        .selected(on);
    let clicked = ui.put(at, toggle).clicked();
    let rule_end = rect.right() - reserve;
    if at.right() + 6. < rule_end {
        ui.painter().hline(
            at.right() + 6.0..=rule_end,
            rect.center().y,
            Stroke::new(1., theme.separator),
        );
    }
    ui.add_space(2.);
    (rect, clicked)
}

/// A section's title, small and muted, with a rule after it up to
/// `reserve` from the right (for something on its row). Returns its row.
pub(super) fn section(ui: &mut Ui, title: &str, reserve: f32) -> Rect {
    section_row(ui, title, reserve, SECTION_TITLE_HEIGHT)
}

/// A section's title on a row `height` tall: as [`section`], for a field
/// on its row.
pub(super) fn section_row(ui: &mut Ui, title: &str, reserve: f32, height: f32) -> Rect {
    ui.add_space(SECTION_GAP);
    let theme = ui.app_theme();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
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
