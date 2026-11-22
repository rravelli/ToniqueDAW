//! Dragging the markers and the loop, and the clip edits they make.

use tonique_engine::{edit::ClipLoop, time::BeatPos};

use crate::{
    app::Tonique,
    bottom::{
        clip::{ClipDrag, ClipScene, LOOP_BAR_HEIGHT, Layout, RULER_HEIGHT},
        paint::{fill, polygon},
    },
    core::clip::{AudioClip, MIN_LENGTH},
    geometry::Rect,
    theme::with_alpha,
};

/// Half the width of the area grabbing a marker or a loop edge.
const HANDLE_REACH: f32 = 4.;
/// The flags marking the clip's start and end in the ruler: their width
/// (pointing inside the clip) and height.
pub(super) const FLAG_SIZE: (f32, f32) = (8., 10.);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum TrimEdge {
    Start,
    End,
}

/// What a drag in the ruler or the lane moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Handle {
    /// Where playback starts in the file: slips the audio.
    StartMarker,
    /// Where the clip ends, without a loop.
    EndMarker,
    LoopStart,
    LoopEnd,
    /// The whole loop.
    Loop,
}

/// Which rows of the view an area covers.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Zone {
    /// The ruler and the lane.
    Full,
    /// A marker's flag, at the ruler's bottom.
    Flag,
    /// The loop's bracket, along the ruler's top.
    LoopBar,
}

/// Where a handle is grabbed, across the view (local to it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Area {
    pub handle: Handle,
    left: f32,
    right: f32,
    zone: Zone,
}

impl Area {
    pub fn rect(&self, layout: Layout) -> Rect {
        let (top, bottom) = match self.zone {
            Zone::Full => (0., layout.lane.1),
            Zone::Flag => (RULER_HEIGHT - FLAG_SIZE.1, RULER_HEIGHT),
            Zone::LoopBar => (0., LOOP_BAR_HEIGHT + 2.),
        };
        Rect::new(self.left, top, self.right - self.left, bottom - top)
    }
}

/// A handle being dragged.
pub(super) struct HandleDrag {
    pub area: Area,
    /// Where it was grabbed, across the view.
    pub press_x: f32,
    /// The loop as it was, to move it by how far it's dragged.
    pub grabbed: Option<ClipLoop>,
}

impl Tonique {
    /// The handle at `(x, y)` in the view, if any: the markers by their
    /// flags (or the lines under them), the loop's bracket by its edges or
    /// its middle.
    pub(super) fn handle_at(
        &self,
        shown: &AudioClip,
        layout: Layout,
        x: f32,
        y: f32,
    ) -> Option<Area> {
        let details = &self.bottom.clip;
        let bpm = self.state.bpm();
        let span = layout.span();
        let at = |file: f64| details.x(file, bpm, span);
        let mut areas = Vec::new();
        let marker = |handle, x: f32| Area {
            handle,
            left: x - HANDLE_REACH,
            right: x + HANDLE_REACH,
            zone: Zone::Full,
        };
        let flag = |handle, edge, x: f32| {
            let left = match edge {
                TrimEdge::Start => x,
                TrimEdge::End => x - FLAG_SIZE.0,
            };
            Area {
                handle,
                left,
                right: left + FLAG_SIZE.0,
                zone: Zone::Flag,
            }
        };
        let start = at(shown.offset);
        areas.push(marker(Handle::StartMarker, start));
        areas.push(flag(Handle::StartMarker, TrimEdge::Start, start));
        match shown.active_loop() {
            Some(looped) => {
                let (left, right) = (at(looped.start_s), at(looped.end_s));
                let bar = |handle, left: f32, right: f32| Area {
                    handle,
                    left,
                    right,
                    zone: Zone::LoopBar,
                };
                // The middle first: the edges, after it, win where they
                // overlap it.
                areas.push(bar(Handle::Loop, left, right));
                areas.push(bar(
                    Handle::LoopStart,
                    left - HANDLE_REACH,
                    left + HANDLE_REACH,
                ));
                areas.push(bar(
                    Handle::LoopEnd,
                    right - HANDLE_REACH,
                    right + HANDLE_REACH,
                ));
            }
            None => {
                let end = at(shown.offset + shown.file_seconds(shown.length, bpm));
                areas.push(marker(Handle::EndMarker, end));
                areas.push(flag(Handle::EndMarker, TrimEdge::End, end));
            }
        }
        areas
            .into_iter()
            .rev()
            .find(|area| area.rect(layout).contains(x, y))
    }

    /// Follows a handle dragged to `pointer_x`: the edit shows while
    /// dragged, and applies as one undo step on release.
    pub(super) fn follow_handle(&mut self, clip: &AudioClip, pointer_x: f32, alt: bool) {
        let Some(ClipDrag::Handle(drag)) = &self.bottom.clip.drag else {
            return;
        };
        let bpm = self.state.bpm();
        let shown = self.state.shown_clip(clip);
        let details = &self.bottom.clip;
        let span = details.layout().span();
        let at = details.file_at_x(pointer_x, bpm, span, alt);
        let edited = match drag.area.handle {
            Handle::StartMarker => {
                let mut slipped = shown.clone();
                slipped.slip_to(at, bpm);
                Some(slipped)
            }
            Handle::EndMarker => {
                let mut trimmed = shown.clone();
                let length = trimmed.file_beats(at - trimmed.offset, bpm).max(MIN_LENGTH);
                trimmed.trim_end_at(trimmed.position + length, bpm);
                Some(trimmed)
            }
            Handle::LoopStart => shown
                .active_loop()
                .map(|looped| with_loop(&shown, at, looped.end_s, bpm)),
            Handle::LoopEnd => shown
                .active_loop()
                .map(|looped| with_loop(&shown, looped.start_s, at, bpm)),
            Handle::Loop => {
                let delta = at - details.file_at_x(drag.press_x, bpm, span, alt);
                drag.grabbed.map(|looped| {
                    let (start, end) = (looped.start_s + delta, looped.end_s + delta);
                    // Kept whole inside the file.
                    let source = shown.source_seconds() as f64;
                    let shift = (-start).max(0.) - (end - source).max(0.);
                    with_loop(&shown, start + shift, end + shift, bpm)
                })
            }
        };
        if let Some(edited) = edited {
            self.state.resize_clip(edited);
        }
    }
}

/// The loop's bracket along the ruler's top, in the clip's colour; the
/// start marker (and the end one, without a loop) across the lane with its
/// flag in the ruler, in the text colour to stand out on the waveform.
pub(super) fn paint_markers(window: &mut gpui::Window, scene: &ClipScene, bounds: Rect) {
    let (left, top) = (bounds.left, bounds.top);
    let lane_bottom = top + scene.layout.lane.1;
    if let Some((loop_left, loop_right)) = scene.loop_x {
        let (loop_left, loop_right) = (left + loop_left, left + loop_right);
        let bar = Rect::new(
            loop_left,
            top + 1.,
            loop_right - loop_left,
            LOOP_BAR_HEIGHT - 1.,
        );
        crate::bottom::paint::fill_rounded(window, bar, 1., scene.color);
        for x in [loop_left, loop_right] {
            fill(
                window,
                Rect::new(x.round(), bar.top, 1., lane_bottom - bar.top),
                with_alpha(scene.color, 160),
            );
        }
    }
    let ink = scene.theme.text;
    let ruler_bottom = top + RULER_HEIGHT;
    let flag_top = ruler_bottom - FLAG_SIZE.1;
    for &(edge, x) in &scene.markers {
        let x = left + x;
        fill(
            window,
            Rect::new(x - 0.75, flag_top, 1.5, lane_bottom - flag_top),
            ink,
        );
        let middle = flag_top + FLAG_SIZE.1 / 2.;
        let points = match edge {
            TrimEdge::Start => [(x, flag_top), (x + FLAG_SIZE.0, middle), (x, ruler_bottom)],
            TrimEdge::End => [(x, flag_top), (x - FLAG_SIZE.0, middle), (x, ruler_bottom)],
        };
        polygon(window, &points, ink);
    }
}

/// `clip` looping `start..end` of its file (at least [`MIN_LENGTH`] long,
/// inside the file), its start marker no later than the loop's end.
pub(super) fn with_loop(clip: &AudioClip, start: f64, end: f64, bpm: f32) -> AudioClip {
    let source = clip.source_seconds() as f64;
    let min = clip.file_seconds(MIN_LENGTH, bpm);
    let start = start.clamp(0., (source - min).max(0.));
    let end = end.clamp(start + min, source.max(start + min));
    let mut looped = clip.clone();
    looped.set_loop(
        Some(ClipLoop {
            start_s: start,
            end_s: end,
        }),
        bpm,
    );
    if looped.offset > end {
        looped.offset = start;
    }
    looped
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

/// `clip` with its end moved to `at`, as the timeline trims it: its sound
/// stays in place. It keeps at least [`MIN_LENGTH`].
pub(super) fn trimmed_end(clip: &AudioClip, at: BeatPos, bpm: f32) -> AudioClip {
    let mut clip = clip.clone();
    clip.trim_end_at(at.max(clip.position + MIN_LENGTH), bpm);
    clip
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_areas_cover_their_rows() {
        let layout = Layout::of(200., 500.);
        let flag = Area {
            handle: Handle::StartMarker,
            left: 10.,
            right: 18.,
            zone: Zone::Flag,
        };
        assert!(flag.rect(layout).contains(12., RULER_HEIGHT - 2.));
        assert!(!flag.rect(layout).contains(12., 2.));
        let marker = Area {
            zone: Zone::Full,
            ..flag
        };
        assert!(marker.rect(layout).contains(12., 150.));
    }
}
