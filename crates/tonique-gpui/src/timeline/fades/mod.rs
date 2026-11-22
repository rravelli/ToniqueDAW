//! A clip's fades and crossfades on the timeline, as the egui app's
//! `clip_fades`: their curves over its waveform, the faded part shaded; a
//! handle at the top of each, shown over the clip, to drag its length; the
//! curve itself to drag its curvature; a menu on either for its shape.
//! Where two clips touch, a handle on the seam drags a crossfade across it.
//!
//! Where things are is worked out here, for the scene and for the pointer
//! alike; [`paint`] draws them, [`drag`] follows the pointer.

mod drag;
mod menus;
pub(super) mod paint;
mod scene;

use std::collections::{HashMap, HashSet};

use tonique_engine::{
    edit::{ClipFades, ClipId, Fade},
    nodes::{FadeCurve, FadeShape},
};

use crate::{
    app::Tonique,
    core::clip::{AudioClip, Crossfades, Seam, clip_seams, crossfades},
    geometry::Rect,
    timeline::rows::{HEADER_HEIGHT, Row, TRIM_HANDLE},
};

pub use drag::FadeDrag;

/// A fade's handle, a square at the top of the clip's body.
const HANDLE: f32 = 8.;
/// Around it, what can be grabbed.
const HANDLE_REACH: f32 = 2.;
/// How near a curve grabs it.
const CURVE_REACH: f32 = 5.;
/// Too small a clip has no handles: its trim handles take it all.
const MIN_WIDTH: f32 = 2. * (TRIM_HANDLE + HANDLE + HANDLE_REACH);
/// A fade narrower than this has no curve to grab.
const MIN_CURVE_WIDTH: f32 = 8.;
/// Between two points of a curve drawn.
const CURVE_STEP: f32 = 2.;
/// A crossfade made by double-clicking a seam, in seconds.
const DEFAULT_CROSSFADE_S: f64 = 0.1;

/// Which end of a clip a fade is at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FadeEnd {
    In,
    Out,
}

impl FadeEnd {
    pub fn heard(self, clip: &AudioClip, crossed: Crossfades, bpm: f32) -> Fade {
        let (fade_in, fade_out) = clip.heard_fades(crossed, bpm);
        match self {
            Self::In => fade_in,
            Self::Out => fade_out,
        }
    }

    /// Whether a crossfade takes its place.
    pub fn crossed(self, crossed: Crossfades) -> bool {
        match self {
            Self::In => crossed.start.is_some(),
            Self::Out => crossed.end.is_some(),
        }
    }

    fn at_seam(self, crossed: Crossfades) -> bool {
        match self {
            Self::In => crossed.start_seam,
            Self::Out => crossed.end_seam,
        }
    }

    pub fn fade(self, fades: &ClipFades) -> Fade {
        match self {
            Self::In => fades.fade_in,
            Self::Out => fades.fade_out,
        }
    }

    pub fn fade_mut(self, fades: &mut ClipFades) -> &mut Fade {
        match self {
            Self::In => &mut fades.fade_in,
            Self::Out => &mut fades.fade_out,
        }
    }

    /// `clip` with this fade set to `seconds`, as far as there's room.
    pub fn set_length(self, clip: &mut AudioClip, seconds: f64, crossed: Crossfades, bpm: f32) {
        match self {
            Self::In => clip.set_fade_in(seconds, crossed, bpm),
            Self::Out => clip.set_fade_out(seconds, crossed, bpm),
        }
    }
}

pub fn shape_name(shape: FadeShape) -> &'static str {
    match shape {
        FadeShape::Linear => "Linear",
        FadeShape::EqualPower => "Equal power",
        FadeShape::Exponential => "Exponential",
    }
}

/// `seconds` as read beside a handle dragged.
fn format_length(seconds: f64) -> String {
    if seconds < 1. {
        format!("{:.0} ms", seconds * 1000.)
    } else {
        format!("{seconds:.2} s")
    }
}

/// A fade's part of the timeline, and its gain over it.
#[derive(Clone, Copy, Debug)]
pub struct FadeSpan {
    pub left: f32,
    pub right: f32,
    pub curve: FadeCurve,
    /// Rises from left to right; otherwise it's a fade out, played
    /// backwards from the right.
    pub rising: bool,
}

impl FadeSpan {
    fn gain(&self, x: f32) -> f32 {
        let width = (self.right - self.left).max(f32::EPSILON);
        let through = if self.rising {
            x - self.left
        } else {
            self.right - x
        };
        self.curve.gain(through / width)
    }

    /// Its points over `body`, between `from` and `to` across.
    pub fn points(&self, body: Rect, from: f32, to: f32) -> Vec<(f32, f32)> {
        let (from, to) = (self.left.max(from), self.right.min(to));
        if from >= to {
            return Vec::new();
        }
        let steps = ((to - from) / CURVE_STEP).ceil().max(1.) as usize;
        (0..=steps)
            .map(|step| {
                let x = from + (to - from) * step as f32 / steps as f32;
                (x, gain_y(body, self.gain(x)))
            })
            .collect()
    }

    /// Whether `(x, y)` is on the curve, inside `area`.
    fn is_near(&self, x: f32, y: f32, area: Rect, body: Rect) -> bool {
        (self.left..=self.right).contains(&x)
            && area.contains(x, y)
            && (y - gain_y(body, self.gain(x))).abs() <= CURVE_REACH
    }
}

fn gain_y(body: Rect, gain: f32) -> f32 {
    body.bottom() - gain * body.height
}

/// Below a clip's header, where its fades are drawn.
pub fn body_of(rect: Rect) -> Rect {
    Rect::new(
        rect.left,
        rect.top + HEADER_HEIGHT,
        rect.width,
        (rect.height - HEADER_HEIGHT).max(0.),
    )
}

/// The fade at `end` of `clip` across `rect`, as heard with `crossed`.
pub fn clip_fade(
    clip: &AudioClip,
    rect: Rect,
    crossed: Crossfades,
    end: FadeEnd,
    bpm: f32,
) -> (Fade, FadeSpan) {
    let fade = end.heard(clip, crossed, bpm);
    let per_second = rect.width / clip.seconds(bpm).max(1e-6) as f32;
    let width = fade.length_s as f32 * per_second;
    let (left, right) = match end {
        FadeEnd::In => (rect.left, rect.left + width),
        FadeEnd::Out => (rect.right() - width, rect.right()),
    };
    let span = FadeSpan {
        left,
        right,
        curve: fade.curve,
        rising: end == FadeEnd::In,
    };
    (fade, span)
}

/// The square a handle is drawn as, centred at `x` near `top`.
fn handle_square(x: f32, top: f32) -> Rect {
    Rect::new(x - HANDLE / 2., top + HANDLE_REACH, HANDLE, HANDLE)
}

/// What can be grabbed of a handle drawn as `square`.
fn handle_reach(square: Rect) -> Rect {
    Rect::new(
        square.left - HANDLE_REACH,
        square.top - HANDLE_REACH,
        square.width + 2. * HANDLE_REACH,
        square.height + 2. * HANDLE_REACH,
    )
}

/// What a press would grab of the fades.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FadePart {
    Handle(FadeEnd),
    Curve(FadeEnd),
    /// A seam's handle: its crossfade's length.
    SeamHandle,
    /// A crossfade's curves.
    SeamCurve,
}

impl FadePart {
    pub fn is_curve(self) -> bool {
        matches!(self, Self::Curve(_) | Self::SeamCurve)
    }
}

/// The fades at the pointer.
#[derive(Clone, Debug)]
pub struct FadeHit {
    pub part: FadePart,
    /// The clip whose fades it changes: at a seam, the later one's.
    pub clip: AudioClip,
    pub crossed: Crossfades,
    pub seam: Option<Seam>,
    /// The body it's drawn over: the clip's, or both clips' at a seam.
    pub body: Rect,
}

/// A clip's fade handle (or curve) to grab.
pub struct ClipHandle {
    pub end: FadeEnd,
    pub square: Rect,
    pub span: FadeSpan,
    /// The fade is wide enough to grab its curve.
    pub has_curve: bool,
}

/// A seam on screen, local to the timeline.
pub struct SeamPlace {
    pub seam: Seam,
    /// Where the clips touch.
    pub x: f32,
    /// Its crossfade's body, from before the seam to after it.
    pub lane: Rect,
    /// The two clips, end to end.
    pub area: Rect,
    pub rising: FadeSpan,
    pub falling: FadeSpan,
    pub square: Rect,
}

impl SeamPlace {
    pub fn crossing(&self) -> bool {
        self.seam.length_s() > 0. && !self.lane.is_empty()
    }

    fn curve_near(&self, x: f32, y: f32) -> bool {
        self.crossing()
            && (x - self.x).abs() > TRIM_HANDLE
            && (self.rising.is_near(x, y, self.lane, self.lane)
                || self.falling.is_near(x, y, self.lane, self.lane))
    }
}

/// A track's clips as they'd sound once a drag of clips is dropped (those
/// dragged away gone), and where they touch.
pub struct TrackFades {
    pub clips: Vec<AudioClip>,
    pub seams: Vec<Seam>,
    pub crossed: HashMap<ClipId, Crossfades>,
}

impl TrackFades {
    pub fn of(&self, id: ClipId) -> Crossfades {
        self.crossed.get(&id).copied().unwrap_or_default()
    }

    fn clip(&self, id: ClipId) -> Option<&AudioClip> {
        self.clips.iter().find(|clip| clip.id == id)
    }
}

/// Too small a lane for handles below a clip's header.
fn lane_too_small(height: f32) -> bool {
    height - HEADER_HEIGHT < 2. * HANDLE
}

impl Tonique {
    /// `row`'s clips, without those `left_behind` by a drag, and their
    /// seams.
    pub(super) fn track_fades(&self, row: &Row, left_behind: &HashSet<ClipId>) -> TrackFades {
        let state = &self.state;
        let clips: Vec<AudioClip> = row
            .track
            .clips
            .iter()
            .filter(|clip| !left_behind.contains(&clip.id))
            .map(|clip| state.shown_clip(clip))
            .collect();
        let seams = clip_seams(&clips, state.bpm());
        let crossed = crossfades(&seams);
        TrackFades {
            clips,
            seams,
            crossed,
        }
    }

    /// The handles of a clip at `rect` (local to the timeline): none on a
    /// collapsed track or too small a clip; at a seam, only to get hold of
    /// a fade there already.
    pub(super) fn clip_handles(
        &self,
        clip: &AudioClip,
        rect: Rect,
        crossed: Crossfades,
        collapsed: bool,
    ) -> Vec<ClipHandle> {
        let body = body_of(rect);
        if collapsed || body.height < 2. * HANDLE || rect.width < MIN_WIDTH {
            return Vec::new();
        }
        let bpm = self.state.bpm();
        let inset = TRIM_HANDLE + HANDLE / 2. + HANDLE_REACH;
        [FadeEnd::In, FadeEnd::Out]
            .into_iter()
            .filter_map(|end| {
                let (fade, span) = clip_fade(clip, rect, crossed, end, bpm);
                let in_the_way = end.at_seam(crossed) && fade.length_s <= 0.;
                if end.crossed(crossed) || in_the_way {
                    return None;
                }
                let x = match end {
                    FadeEnd::In => span.right,
                    FadeEnd::Out => span.left,
                }
                .clamp(rect.left + inset, rect.right() - inset);
                Some(ClipHandle {
                    end,
                    square: handle_square(x, body.top),
                    span,
                    has_curve: span.right - span.left >= MIN_CURVE_WIDTH,
                })
            })
            .collect()
    }

    /// Where `seam` is on a lane at `top`, `height` high, if both its clips
    /// are in `fades`.
    pub(super) fn seam_place(
        &self,
        seam: &Seam,
        fades: &TrackFades,
        top: f32,
        height: f32,
    ) -> Option<SeamPlace> {
        let (earlier, later) = (fades.clip(seam.earlier)?, fades.clip(seam.later)?);
        let state = &self.state;
        let (viewport, bpm) = (self.viewport(), state.bpm());
        let per_second = state.grid.pixels_per_beat() * bpm / 60.;
        let x = state.grid.beats_to_x(seam.at, viewport);
        let left = x - seam.before_s as f32 * per_second;
        let right = x + seam.after_s as f32 * per_second;
        let body_top = top + HEADER_HEIGHT;
        let lane = Rect::new(
            left,
            body_top,
            right - left,
            (height - HEADER_HEIGHT).max(0.),
        );
        let start = state.grid.beats_to_x(earlier.position, viewport);
        let end = state.grid.beats_to_x(later.end(bpm), viewport);
        let area = Rect::new(start, top, end - start, height);
        let handle_x = if seam.length_s() <= 0. {
            x
        } else if seam.after_s >= seam.before_s {
            right
        } else {
            left
        };
        let span = |curve, rising| FadeSpan {
            left,
            right,
            curve,
            rising,
        };
        Some(SeamPlace {
            seam: *seam,
            x,
            lane,
            area,
            rising: span(seam.curve, true),
            falling: span(seam.curve.crossing(), false),
            square: handle_square(handle_x, body_top),
        })
    }

    /// The fades under `(local_x, local_y)`: a seam's handle or curves
    /// first, over the clips, then a clip's handles, then its curves.
    pub(super) fn fade_at(&self, rows: &[Row], local_x: f32, local_y: f32) -> Option<FadeHit> {
        let row = &rows[self.row_at(rows, local_y)?];
        if row.is_group() || row.track.collapsed {
            return None;
        }
        let fades = self.track_fades(row, &self.left_behind());
        let (top, height) = (self.row_top(row), row.track.height);
        if !lane_too_small(height) {
            for seam in &fades.seams {
                let Some(place) = self.seam_place(seam, &fades, top, height) else {
                    continue;
                };
                if !place.area.contains(local_x, local_y) {
                    continue;
                }
                let part = if handle_reach(place.square).contains(local_x, local_y) {
                    FadePart::SeamHandle
                } else if place.curve_near(local_x, local_y) {
                    FadePart::SeamCurve
                } else {
                    continue;
                };
                let later = fades.clip(seam.later)?.clone();
                return Some(FadeHit {
                    part,
                    crossed: fades.of(later.id),
                    clip: later,
                    seam: Some(*seam),
                    body: place.lane,
                });
            }
        }
        // The topmost clip under the pointer.
        let clip = fades.clips.iter().rev().find(|clip| {
            self.clip_rect_at(clip, top, height)
                .contains(local_x, local_y)
        })?;
        let rect = self.clip_rect_at(clip, top, height);
        let crossed = fades.of(clip.id);
        let handles = self.clip_handles(clip, rect, crossed, false);
        let body = body_of(rect);
        let area = Rect::new(
            body.left + TRIM_HANDLE,
            body.top,
            (body.width - 2. * TRIM_HANDLE).max(0.),
            body.height,
        );
        let part = handles
            .iter()
            .find(|handle| handle_reach(handle.square).contains(local_x, local_y))
            .map(|handle| FadePart::Handle(handle.end))
            .or_else(|| {
                handles
                    .iter()
                    .find(|handle| {
                        handle.has_curve && handle.span.is_near(local_x, local_y, area, body)
                    })
                    .map(|handle| FadePart::Curve(handle.end))
            })?;
        Some(FadeHit {
            part,
            clip: clip.clone(),
            crossed,
            seam: None,
            body,
        })
    }
}

#[cfg(test)]
mod tests;
