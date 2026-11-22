//! The clip's properties, left of its view: panels side by side, each
//! shown or hidden from the rail on their left. Positions are dragged
//! sideways, clicked and stepped by the arrow keys, or double-clicked and
//! typed; edits show while dragged and apply as one undo step once released
//! (or stepped, or typed).

use gpui::{
    AnyElement, Context, CursorStyle, MouseButton, MouseDownEvent, Stateful, Window, div,
    prelude::*, px,
};
use tonique_engine::time::{BeatPos, TimeSignature};

use crate::{
    app::{ArrowHolder, Tonique},
    bottom::clip::{
        ClipArrows, ClipDrag,
        markers::{default_loop, trimmed_end, with_loop},
    },
    core::clip::{AudioClip, MIN_LENGTH},
    icons,
    theme::{Color, Theme, hsla, size},
    utils::{
        format_bars, format_time, parse_bars, parse_parts,
        steps::{self, KeyStep, Presses},
    },
    widgets::{Flat, TextField, glyph, typing_view},
};

/// Between the inspector's cells, side by side.
pub(super) const CELL_GAP: f32 = 4.;
/// Above each section.
const SECTION_GAP: f32 = 4.;
/// The inspector's values.
pub(super) const FIELD_HEIGHT: f32 = 18.;
/// The column of panel toggles, left of the panels.
pub(super) const RAIL_WIDTH: f32 = 24.;
const PANEL_WIDTH: f32 = 176.;
/// Between the rail and the panels, and between panels: a rule in its
/// middle.
const PANEL_GAP: f32 = 9.;
/// A value dragged this far is being dragged, not clicked.
const DRAG_THRESHOLD: f32 = 2.;
/// A file's tempo, as typed.
const BPM_RANGE: std::ops::RangeInclusive<f64> = 10.0..=999.0;

/// The panels shown: the last picked, whichever clip.
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
    /// The inspector's width: its rail and the panels shown.
    pub(super) fn width(self) -> f32 {
        let shown = self.clip as u8 + self.audio as u8;
        RAIL_WIDTH + shown as f32 * (PANEL_GAP + PANEL_WIDTH)
    }
}

/// A position of the clip panel's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Field {
    Start,
    End,
    Length,
    /// Where in its file it starts.
    Offset,
    LoopPosition,
    LoopLength,
}

/// What's being typed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Typed {
    Field(Field),
    /// The file's tempo.
    Bpm,
}

impl Field {
    /// Its value for `clip`: beats, or seconds of the file.
    fn value(self, clip: &AudioClip, bpm: f32) -> f64 {
        let looped = clip
            .active_loop()
            .unwrap_or_else(|| default_loop(clip, bpm));
        match self {
            Self::Start => clip.position.0,
            Self::End => clip.end(bpm).0,
            Self::Length => clip.end(bpm).0 - clip.position.0,
            Self::Offset => clip.offset,
            Self::LoopPosition => looped.start_s,
            Self::LoopLength => clip.file_beats(looped.end_s - looped.start_s, bpm),
        }
    }

    /// How far the arrow keys step it: a sixteenth (a bar with Shift), or
    /// 10 ms (a second).
    fn key_step(self, signature: TimeSignature) -> KeyStep {
        match self {
            Self::Offset | Self::LoopPosition => steps::SECONDS,
            _ => steps::beats(signature),
        }
    }

    /// How much a pixel's drag changes it.
    fn speed(self) -> f64 {
        match self {
            Self::Offset | Self::LoopPosition => 0.005,
            _ => 1. / 16.,
        }
    }

    fn format(self, value: f64, signature: TimeSignature) -> String {
        match self {
            Self::Start | Self::End => format_bars(value, signature),
            Self::Length | Self::LoopLength => format_length(value, signature),
            Self::Offset | Self::LoopPosition => format_time(value),
        }
    }

    fn parse(self, text: &str, signature: TimeSignature) -> Option<f64> {
        match self {
            Self::Start | Self::End => parse_bars(text, signature),
            Self::Length | Self::LoopLength => parse_length(text, signature),
            Self::Offset | Self::LoopPosition => parse_time(text),
        }
    }

    /// `clip` with this set to `value`; positions and lengths snap to
    /// sixteenths unless `free`.
    fn apply(self, clip: &AudioClip, value: f64, bpm: f32, free: bool) -> AudioClip {
        let snap = |beats: f64| {
            if free {
                beats
            } else {
                (beats * 4.).round() / 4.
            }
        };
        let start = clip.position.0;
        let looped = clip
            .active_loop()
            .unwrap_or_else(|| default_loop(clip, bpm));
        match self {
            Self::Start => {
                let mut moved = clip.clone();
                moved.position = BeatPos(snap(value).max(0.));
                moved
            }
            Self::End => trimmed_end(clip, BeatPos(snap(value).max(start + MIN_LENGTH)), bpm),
            Self::Length => trimmed_end(clip, clip.position + snap(value).max(MIN_LENGTH), bpm),
            Self::Offset => {
                let mut slipped = clip.clone();
                slipped.slip_to(value.max(0.), bpm);
                slipped
            }
            Self::LoopPosition => {
                let source = clip.source_seconds() as f64;
                with_loop(clip, value.clamp(0., source), looped.end_s, bpm)
            }
            Self::LoopLength => {
                let end = looped.start_s + clip.file_seconds(snap(value).max(MIN_LENGTH), bpm);
                with_loop(clip, looped.start_s, end, bpm)
            }
        }
    }
}

impl Tonique {
    pub(super) fn inspector_view(
        &self,
        clip: &AudioClip,
        color: Color,
        width: f32,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let theme = crate::theme::current(context);
        let panels = self.bottom.clip.panels;
        let rail = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(2.))
            .w(px(RAIL_WIDTH))
            .child(
                Flat::ghost("panel-clip", RAIL_WIDTH)
                    .selected(panels.clip)
                    .show(
                        glyph(icons::BROWSER, size::HEADING, false),
                        &theme,
                        context,
                        |this, _| this.bottom.clip.panels.clip ^= true,
                    ),
            )
            .child(
                Flat::ghost("panel-audio", RAIL_WIDTH)
                    .selected(panels.audio)
                    .show(
                        glyph(icons::WAVEFORM, size::HEADING, false),
                        &theme,
                        context,
                        |this, _| this.bottom.clip.panels.audio ^= true,
                    ),
            );
        let shown = self.state.shown_clip(clip);
        let mut row = div()
            .id("clip-panels")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .overflow_x_scroll();
        let mut parts: Vec<(&'static str, AnyElement)> = Vec::new();
        if panels.clip {
            parts.push(("clip-panel", self.clip_panel(&shown, &theme, context)));
        }
        if panels.audio {
            parts.push((
                "audio-panel",
                self.audio_panel(&shown, color, &theme, context),
            ));
        }
        for (id, content) in parts {
            row = row
                .child(
                    div()
                        .flex_none()
                        .w(px(1.))
                        .h_full()
                        .mx(px((PANEL_GAP - 1.) / 2.))
                        .bg(hsla(theme.separator)),
                )
                .child(
                    div()
                        .id(id)
                        .flex_none()
                        .w(px(PANEL_WIDTH))
                        .h_full()
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .text_size(px(size::LABEL))
                        .child(content),
                );
        }
        div()
            .flex()
            .flex_none()
            .w(px(width))
            .h_full()
            .overflow_hidden()
            .child(rail)
            .child(row)
            .into_any_element()
    }

    /// Where the clip is (start, end, length), where in its file it starts
    /// (offset), and its loop, switched on its title's row.
    fn clip_panel(
        &self,
        shown: &AudioClip,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let field = |this: &Self, label, field: Field, enabled, context: &mut Context<Self>| {
            this.field_cell(label, field, shown, enabled, theme, context)
        };
        let looped = shown.active_loop().is_some();
        let id = shown.id;
        let toggle = toggle_section(
            "loop-toggle",
            "Loop",
            looped,
            true,
            theme,
            context,
            move |this| {
                let Some((_, clip)) = this.state.clip_with_track(id) else {
                    return;
                };
                let bpm = this.state.bpm();
                let mut toggled = clip.clone();
                match clip.active_loop() {
                    Some(_) => toggled.set_loop(None, bpm),
                    None => toggled.set_loop(Some(default_loop(&clip, bpm)), bpm),
                }
                this.state.commit_resize_clip(toggled);
            },
        );
        div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .child(section("Clip", theme))
            .child(cells(
                field(self, "Start", Field::Start, true, context),
                field(self, "End", Field::End, true, context),
            ))
            .child(div().h(px(CELL_GAP)))
            .child(cells(
                field(self, "Length", Field::Length, true, context),
                field(self, "Offset", Field::Offset, true, context),
            ))
            .child(toggle)
            // Off, it shows where it would be, greyed out.
            .child(cells(
                field(self, "Position", Field::LoopPosition, looped, context),
                field(self, "Length", Field::LoopLength, looped, context),
            ))
            .into_any_element()
    }

    /// A position's label above its value: dragged sideways, clicked to
    /// take the arrow keys, or double-clicked and typed.
    #[allow(clippy::too_many_arguments)]
    fn field_cell(
        &self,
        label: &'static str,
        field: Field,
        shown: &AudioClip,
        enabled: bool,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let bpm = self.state.bpm();
        let signature = self.state.time_signature();
        let value = field.value(shown, bpm);
        let typing = match &self.bottom.clip.typing {
            Some((Typed::Field(typed), text)) if *typed == field => Some(text),
            _ => None,
        };
        let value_element = match typing {
            Some(text) => typing_view(
                "typing",
                text,
                theme,
                context,
                |this| this.bottom.clip.typing.as_mut().map(|(_, text)| text),
                Self::finish_typing,
            )
            .h(px(FIELD_HEIGHT))
            .into_any_element(),
            None => {
                let holder = ArrowHolder::Clip(ClipArrows::Field(field));
                let accent = hsla(theme.accent);
                let element = value_box(
                    ("field", field as usize),
                    field.format(value, signature),
                    enabled,
                    theme,
                )
                .when(self.holds_arrows(&holder), |element| {
                    element.border_1().border_color(accent)
                });
                let id = shown.id;
                if enabled {
                    element
                        .on_mouse_down(
                            MouseButton::Left,
                            context.listener(
                                move |this, event: &MouseDownEvent, window, context| {
                                    context.stop_propagation();
                                    this.hold_arrows(holder.clone());
                                    if event.click_count >= 2 {
                                        this.bottom.clip.drag = None;
                                        if let Some((_, clip)) = this.state.clip_with_track(id) {
                                            this.type_field(field, &clip, window, context);
                                        }
                                        context.notify();
                                        return;
                                    }
                                    this.bottom.clip.drag = Some(ClipDrag::Field {
                                        field,
                                        start_x: f32::from(event.position.x),
                                        start: value,
                                        moved: false,
                                    });
                                    context.notify();
                                },
                            ),
                        )
                        .into_any_element()
                } else {
                    element.into_any_element()
                }
            }
        };
        cell(label, value_element, enabled, theme)
    }

    /// Follows a field dragged to `pointer_x`; Alt keeps it off the grid.
    pub(super) fn follow_field(&mut self, clip: &AudioClip, pointer_x: f32, alt: bool) {
        let Some(ClipDrag::Field {
            field,
            start_x,
            start,
            moved,
        }) = self.bottom.clip.drag.as_mut()
        else {
            return;
        };
        let moved_by = pointer_x - *start_x;
        *moved |= moved_by.abs() > DRAG_THRESHOLD;
        if !*moved {
            return;
        }
        let value = *start + moved_by as f64 * field.speed();
        let field = *field;
        let bpm = self.state.bpm();
        let shown = self.state.shown_clip(clip);
        self.state.resize_clip(field.apply(&shown, value, bpm, alt));
    }

    /// `field` stepped by `presses` of the arrow keys, as one undo step;
    /// positions and lengths stay on sixteenths.
    pub(super) fn step_field(&mut self, clip: &AudioClip, field: Field, presses: Presses) {
        let bpm = self.state.bpm();
        let value = field.value(clip, bpm);
        let step = field.key_step(self.state.time_signature());
        let edited = field.apply(clip, value + presses.amount(step), bpm, false);
        if field.value(&edited, bpm) != value {
            self.state.commit_resize_clip(edited);
        }
    }

    /// Starts typing `field`'s value.
    pub(super) fn type_field(
        &mut self,
        field: Field,
        clip: &AudioClip,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let value = field.value(clip, self.state.bpm());
        let text = field.format(value, self.state.time_signature());
        let mut typed = TextField::new(context);
        typed.start(text, window, context);
        self.bottom.clip.typing = Some((Typed::Field(field), typed));
    }

    /// Starts typing the file's tempo.
    pub(super) fn type_bpm(
        &mut self,
        clip: &AudioClip,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let mut typed = TextField::new(context);
        typed.start(format!("{:.2}", clip.bpm), window, context);
        self.bottom.clip.typing = Some((Typed::Bpm, typed));
    }

    /// Ends typing: applies what was typed when `keep` and it reads.
    pub(super) fn finish_typing(
        &mut self,
        keep: bool,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let typing = self.bottom.clip.typing.take();
        window.focus(&self.focus, context);
        let (Some((typed, text)), Some(id), true) = (typing, self.bottom.clip.clip, keep) else {
            return;
        };
        let Some((_, clip)) = self.state.clip_with_track(id) else {
            return;
        };
        let bpm = self.state.bpm();
        match typed {
            Typed::Field(field) => {
                if let Some(value) = field.parse(&text.text, self.state.time_signature()) {
                    self.state
                        .commit_resize_clip(field.apply(&clip, value, bpm, false));
                }
            }
            Typed::Bpm => {
                if let Ok(file_bpm) = text.text.trim().parse::<f64>()
                    && BPM_RANGE.contains(&file_bpm)
                {
                    let mut edited = clip.clone();
                    edited.set_warp(clip.warp, file_bpm, clip.transpose, clip.detune, bpm);
                    self.state.commit_resize_clip(edited);
                }
            }
        }
    }
}

/// A value in a box as wide as its cell, dimmed when not `enabled`.
pub(super) fn value_box(
    id: impl Into<gpui::ElementId>,
    text: String,
    enabled: bool,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let element = div()
        .id(id.into())
        .flex()
        .items_center()
        .justify_center()
        .w_full()
        .h(px(FIELD_HEIGHT))
        .rounded(px(2.))
        .overflow_hidden()
        .whitespace_nowrap()
        .bg(hsla(theme.bg_control))
        .child(text);
    if enabled {
        element
            .cursor(CursorStyle::ResizeLeftRight)
            .hover(|style| style.bg(hsla(theme.bg_control_hover)))
    } else {
        element
    }
}

/// Two cells side by side, sharing the width.
pub(super) fn cells(first: impl IntoElement, second: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .gap(px(CELL_GAP))
        .child(div().flex_1().min_w_0().child(first))
        .child(div().flex_1().min_w_0().child(second))
}

/// A cell: its label, small and muted, above its value; half strength
/// when not `enabled`.
pub(super) fn cell(
    label: &'static str,
    value: impl IntoElement,
    enabled: bool,
    theme: &Theme,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(1.))
        .when(!enabled, |element| element.opacity(0.5))
        .child(
            div()
                .text_size(px(size::SMALL))
                .text_color(hsla(theme.text_muted))
                .overflow_hidden()
                .whitespace_nowrap()
                .child(label),
        )
        .child(value)
}

/// A rule filling the rest of a section's row.
fn rule(theme: &Theme) -> gpui::Div {
    div()
        .flex_1()
        .min_w_0()
        .ml(px(6.))
        .h(px(1.))
        .bg(hsla(theme.separator))
}

/// A section's title, small and muted, with a rule after it.
pub(super) fn section(title: &str, theme: &Theme) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .mt(px(SECTION_GAP))
        .mb(px(2.))
        .h(px(14.))
        .child(
            div()
                .flex_none()
                .text_size(px(size::SMALL))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(hsla(theme.text_muted))
                .child(title.to_uppercase()),
        )
        .child(rule(theme))
}

/// A section switched on and off by its title, a toggle as tall as a
/// field, with a rule after it.
pub(super) fn toggle_section(
    id: &'static str,
    title: &str,
    on: bool,
    enabled: bool,
    theme: &Theme,
    context: &mut Context<Tonique>,
    action: impl Fn(&mut Tonique) + 'static,
) -> gpui::Div {
    let label = div()
        .text_size(px(size::SMALL))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(title.to_uppercase());
    let toggle = Flat::new(id, FIELD_HEIGHT)
        .fit(5.)
        .selected(on)
        .enabled(enabled)
        .show(label, theme, context, move |this, _| action(this));
    div()
        .flex()
        .items_center()
        .mt(px(SECTION_GAP))
        .mb(px(2.))
        .child(toggle)
        .child(rule(theme))
}

/// A length as typed, `bars.beats.sixteenths` (`1.2.0`; missing parts are
/// none), in beats.
pub(super) fn parse_length(text: &str, signature: TimeSignature) -> Option<f64> {
    let parts = parse_parts(text)?;
    let part = |index: usize| parts.get(index).copied().unwrap_or(0.);
    if parts.is_empty() || parts.iter().any(|value| *value < 0.) {
        return None;
    }
    Some(part(0) * signature.beats_per_bar() + part(1) * signature.beat_length() + part(2) / 4.)
}

/// A length as `bars.beats.sixteenths`, counting from 0: one bar and two
/// beats is `1.2.0`.
pub(super) fn format_length(beats: f64, signature: TimeSignature) -> String {
    let sixteenths = (beats * 4.).round().max(0.) as u64;
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
        None => text.parse().ok().filter(|seconds: &f64| *seconds >= 0.),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_and_times_read_back() {
        let signature = TimeSignature::default();
        assert_eq!(format_length(6., signature), "1.2.0");
        assert_eq!(parse_length("1.2.0", signature), Some(6.));
        assert_eq!(parse_length("2", signature), Some(8.));
        assert_eq!(parse_time("1:23.25"), Some(83.25));
        assert_eq!(parse_time("4.5"), Some(4.5));
        assert_eq!(parse_time("-1"), None);
    }

    #[test]
    fn the_inspector_widens_with_its_panels() {
        let both = Panels::default();
        let one = Panels {
            audio: false,
            ..both
        };
        assert_eq!(both.width() - one.width(), PANEL_GAP + PANEL_WIDTH);
    }
}
