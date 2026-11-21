//! Dragging the markers and the loop, and the clip edits they make.

use egui::{Color32, CursorIcon, Rect, Sense, Shape, Stroke, Ui, pos2};
use tonique_engine::{edit::ClipLoop, time::BeatPos};

use crate::{
    core::{
        clip::{AudioClip, MIN_LENGTH},
        state::ProjectState,
    },
    ui::theme::{Theme, ThemeExt, with_alpha},
};

use super::{ClipDetails, LOOP_BAR_HEIGHT};

/// Half the width of the area grabbing a marker or a loop edge.
const HANDLE_REACH: f32 = 4.;
/// The flags marking the clip's start and end in the ruler: their width
/// (pointing inside the clip) and height.
pub(super) const FLAG_SIZE: egui::Vec2 = egui::vec2(8., 10.);

#[derive(Clone, Copy, Debug, PartialEq, Hash)]
pub(super) enum TrimEdge {
    Start,
    End,
}

/// What a drag in the ruler or the lane moves.
#[derive(Clone, Copy, Debug, PartialEq, Hash)]
enum Handle {
    /// Where playback starts in the file: slips the audio.
    StartMarker,
    /// Where the clip ends, without a loop.
    EndMarker,
    LoopStart,
    LoopEnd,
    /// The whole loop.
    Loop,
}

impl ClipDetails {
    /// Drag the markers by their flags (or the lines under them), the loop's
    /// bracket by its edges or its middle. Each edit shows while dragged
    /// and applies as one undo step on release.
    pub(super) fn handles(
        &self,
        ui: &Ui,
        ruler: Rect,
        lane: Rect,
        clip: &AudioClip,
        state: &mut ProjectState,
    ) {
        let bpm = state.bpm();
        let shown = state.shown_clip(clip);
        let view = ruler.union(lane);
        let mut areas: Vec<(Handle, Rect)> = Vec::new();
        let start = self.x(shown.offset, bpm, lane);
        let marker =
            |x: f32| Rect::from_x_y_ranges(x - HANDLE_REACH..=x + HANDLE_REACH, view.y_range());
        areas.push((Handle::StartMarker, marker(start)));
        areas.push((
            Handle::StartMarker,
            flag_rect(TrimEdge::Start, start, ruler),
        ));
        match shown.active_loop() {
            Some(l) => {
                let (left, right) = (self.x(l.start_s, bpm, lane), self.x(l.end_s, bpm, lane));
                let bar = |range: std::ops::RangeInclusive<f32>| {
                    Rect::from_x_y_ranges(range, ruler.top()..=ruler.top() + LOOP_BAR_HEIGHT + 2.)
                };
                // The middle first: the edges, registered after it, win where
                // they overlap it.
                areas.push((Handle::Loop, bar(left..=right)));
                areas.push((
                    Handle::LoopStart,
                    bar(left - HANDLE_REACH..=left + HANDLE_REACH),
                ));
                areas.push((
                    Handle::LoopEnd,
                    bar(right - HANDLE_REACH..=right + HANDLE_REACH),
                ));
            }
            None => {
                let end = self.x(
                    shown.offset + shown.file_seconds(shown.length, bpm),
                    bpm,
                    lane,
                );
                areas.push((Handle::EndMarker, marker(end)));
                areas.push((Handle::EndMarker, flag_rect(TrimEdge::End, end, ruler)));
            }
        }
        // Registered after the lane and the ruler, they win over them.
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let press = ui.input(|i| i.pointer.press_origin());
        for (i, (handle, area)) in areas.iter().enumerate() {
            let response = ui
                .interact(
                    area.intersect(view),
                    ui.id().with(("handle", i)),
                    Sense::drag(),
                )
                .on_hover_and_drag_cursor(match handle {
                    Handle::Loop => CursorIcon::Grab,
                    _ => CursorIcon::ResizeHorizontal,
                });
            if response.hovered() || response.dragged() {
                let theme = ui.app_theme();
                ui.painter_at(view)
                    .rect_filled(*area, 1., theme.hover_overlay);
            }
            if response.drag_started() {
                // The loop as it was, to move it by how far it's dragged.
                ui.data_mut(|d| d.insert_temp(response.id, shown.looped));
            }
            if response.dragged()
                && let Some(pointer) = pointer
            {
                let at = self.file_at_x(ui, pointer.x, bpm, lane);
                let moved_loop = || {
                    let from = press.map_or(pointer.x, |p| p.x);
                    let delta = self.file_at_x(ui, pointer.x, bpm, lane)
                        - self.file_at_x(ui, from, bpm, lane);
                    let grabbed: Option<ClipLoop> = ui.data(|d| d.get_temp(response.id)).flatten();
                    grabbed.map(|l| (l.start_s + delta, l.end_s + delta))
                };
                let edited = match handle {
                    Handle::StartMarker => {
                        let mut c = shown.clone();
                        c.slip_to(at, bpm);
                        Some(c)
                    }
                    Handle::EndMarker => {
                        let mut c = shown.clone();
                        let length = c.file_beats(at - c.offset, bpm).max(MIN_LENGTH);
                        c.trim_end_at(c.position + length, bpm);
                        Some(c)
                    }
                    Handle::LoopStart => shown
                        .active_loop()
                        .map(|l| with_loop(&shown, at, l.end_s, bpm)),
                    Handle::LoopEnd => shown
                        .active_loop()
                        .map(|l| with_loop(&shown, l.start_s, at, bpm)),
                    Handle::Loop => moved_loop().map(|(start, end)| {
                        // Kept whole inside the file.
                        let source = shown.source_seconds() as f64;
                        let shift = (-start).max(0.) - (end - source).max(0.);
                        with_loop(&shown, start + shift, end + shift, bpm)
                    }),
                };
                if let Some(edited) = edited {
                    state.resize_clip(edited);
                }
                if *handle == Handle::Loop {
                    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                }
            }
            if response.drag_stopped() {
                state.commit_resize_clip(state.shown_clip(clip));
            }
        }
    }

    /// The start marker (and the end one, without a loop) across the lane
    /// with its flag in the ruler, in the text colour to stand out on the
    /// waveform; the loop's bracket along the ruler's top, in the clip's
    /// colour.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_markers(
        &self,
        ui: &Ui,
        ruler: Rect,
        lane: Rect,
        clip: &AudioClip,
        color: Color32,
        bpm: f32,
        theme: &Theme,
    ) {
        let view = ruler.union(lane);
        let painter = ui.painter_at(view);
        if let Some(l) = clip.active_loop() {
            let (left, right) = (self.x(l.start_s, bpm, lane), self.x(l.end_s, bpm, lane));
            let bar = Rect::from_x_y_ranges(
                left..=right,
                ruler.top() + 1.0..=ruler.top() + LOOP_BAR_HEIGHT,
            );
            painter.rect_filled(bar, 1., color);
            for x in [left, right] {
                painter.vline(
                    x,
                    bar.top()..=lane.bottom(),
                    Stroke::new(1., with_alpha(color, 160)),
                );
            }
        }
        let mut markers = vec![(TrimEdge::Start, clip.offset)];
        if clip.active_loop().is_none() {
            markers.push((
                TrimEdge::End,
                clip.offset + clip.file_seconds(clip.length, bpm),
            ));
        }
        let ink = theme.text;
        for (edge, at) in markers {
            let x = self.x(at, bpm, lane);
            painter.vline(x, ruler.bottom()..=lane.bottom(), Stroke::new(1.5, ink));
            let flag = flag_rect(edge, x, ruler);
            let middle = egui::vec2(0., FLAG_SIZE.y / 2.);
            let (tip, top, bottom) = match edge {
                TrimEdge::Start => (
                    flag.right_top() + middle,
                    flag.left_top(),
                    flag.left_bottom(),
                ),
                TrimEdge::End => (
                    flag.left_top() + middle,
                    flag.right_top(),
                    flag.right_bottom(),
                ),
            };
            painter.add(Shape::convex_polygon(
                vec![top, tip, bottom],
                ink,
                Stroke::NONE,
            ));
            painter.vline(
                x,
                flag.y_range().min..=ruler.bottom(),
                Stroke::new(1.5, ink),
            );
        }
    }
}

/// `clip` looping `start..end` of its file (at least [`MIN_LENGTH`] long,
/// inside the file), its start marker no later than the loop's end.
pub(super) fn with_loop(clip: &AudioClip, start: f64, end: f64, bpm: f32) -> AudioClip {
    let source = clip.source_seconds() as f64;
    let min = clip.file_seconds(MIN_LENGTH, bpm);
    let start = start.clamp(0., (source - min).max(0.));
    let end = end.clamp(start + min, source.max(start + min));
    let mut c = clip.clone();
    c.set_loop(
        Some(ClipLoop {
            start_s: start,
            end_s: end,
        }),
        bpm,
    );
    if c.offset > end {
        c.offset = start;
    }
    c
}

/// The loop to start with: what the clip plays, or the whole file when
/// that's too short.
pub(super) fn default_loop(clip: &AudioClip, bpm: f32) -> ClipLoop {
    let source = clip.source_seconds() as f64;
    let end = (clip.offset + clip.file_seconds(clip.length, bpm)).min(source);
    if end - clip.offset >= clip.file_seconds(MIN_LENGTH, bpm) {
        ClipLoop {
            start_s: clip.offset,
            end_s: end,
        }
    } else {
        ClipLoop {
            start_s: 0.,
            end_s: source,
        }
    }
}

/// Where the flag of `edge`, drawn at `x`, sits in the ruler: at its bottom,
/// on the clip's side of the edge.
pub(super) fn flag_rect(edge: TrimEdge, x: f32, ruler: Rect) -> Rect {
    let top = ruler.bottom() - FLAG_SIZE.y;
    let left = match edge {
        TrimEdge::Start => x,
        TrimEdge::End => x - FLAG_SIZE.x,
    };
    Rect::from_min_size(pos2(left, top), FLAG_SIZE)
}

/// `clip` with `edge` moved to `at`, as the timeline trims it: its sound
/// stays in place. It keeps at least [`MIN_LENGTH`].
pub(super) fn trimmed(clip: &AudioClip, edge: TrimEdge, at: BeatPos, bpm: f32) -> AudioClip {
    let mut clip = clip.clone();
    match edge {
        TrimEdge::Start => clip.trim_start_at(at.min(clip.end(bpm) - MIN_LENGTH), bpm),
        TrimEdge::End => clip.trim_end_at(at.max(clip.position + MIN_LENGTH), bpm),
    }
    clip
}
