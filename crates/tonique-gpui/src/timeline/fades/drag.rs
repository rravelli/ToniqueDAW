//! Dragging fades: a handle sets a fade's length, a curve its curvature,
//! a seam's handle how far its crossfade reaches, wherever the pointer is
//! (not snapped to the grid). Shown as dragged, applied as one undo step
//! once released.

use tonique_engine::{
    edit::{ClipFades, ClipId, Fade},
    nodes::FadeCurve,
};

use crate::{
    app::Tonique,
    core::clip::AudioClip,
    geometry::Rect,
    timeline::fades::{DEFAULT_CROSSFADE_S, FadeEnd, FadeHit, FadePart, format_length},
};

/// A fade's handle or curve held down.
pub struct FadeDrag {
    pub hit: FadeHit,
    /// Where it was pressed, local to the timeline.
    start_y: f32,
    /// The clip as dragged so far.
    pub current: Option<AudioClip>,
    /// What's shown beside the pointer: the length or the curvature.
    pub label: Option<String>,
}

/// The curvature `curve` gets dragged `moved_by` down over `body`: up
/// bends it up.
fn dragged_curvature(curve: FadeCurve, moved_by: f32, body: Rect) -> FadeCurve {
    let by = -2. * moved_by / body.height.max(1.);
    curve.with_curvature(curve.curvature + by)
}

fn format_curvature(curve: FadeCurve) -> String {
    format!("{:+.0}%", curve.curvature * 100.)
}

impl Tonique {
    /// A press on a fade: a drag, unless it's a double-click on a seam
    /// with no crossfade, which makes one.
    pub(in crate::timeline) fn press_fade(
        &mut self,
        hit: FadeHit,
        local_y: f32,
        click_count: usize,
    ) -> Option<FadeDrag> {
        if let (FadePart::SeamHandle, Some(seam), true) = (hit.part, hit.seam, click_count >= 2)
            && seam.length_s() <= 0.
        {
            let (before, after) = seam.room.window(DEFAULT_CROSSFADE_S);
            let mut edited = hit.clip.clone();
            edited.fades.crossfade.length_s = before + after;
            self.state.commit_resize_clip(edited);
            return None;
        }
        Some(FadeDrag {
            hit,
            start_y: local_y,
            current: None,
            label: None,
        })
    }

    /// Follows the pointer at `(local_x, local_y)` with `drag`.
    pub(in crate::timeline) fn follow_fade(
        &mut self,
        drag: &mut FadeDrag,
        local_x: f32,
        local_y: f32,
    ) {
        let state = &self.state;
        let bpm = state.bpm();
        let hit = &drag.hit;
        let original = &hit.clip;
        let mut edited = original.clone();
        let label = match hit.part {
            FadePart::Handle(end) => {
                let beats = self.beats_at(local_x);
                let per_beat = 60. / bpm as f64;
                let seconds = match end {
                    FadeEnd::In => (beats - original.position) * per_beat,
                    FadeEnd::Out => (original.end(bpm) - beats) * per_beat,
                };
                end.set_length(&mut edited, seconds, hit.crossed, bpm);
                format_length(end.fade(&edited.fades).length_s)
            }
            FadePart::Curve(end) => {
                let curve = &mut end.fade_mut(&mut edited.fades).curve;
                *curve = dragged_curvature(*curve, local_y - drag.start_y, hit.body);
                format_curvature(*curve)
            }
            FadePart::SeamHandle => {
                let Some(seam) = hit.seam else { return };
                let beats = self.beats_at(local_x);
                let distance = (beats - seam.at).abs() * 60. / bpm as f64;
                let length = seam.room.length_reaching(distance);
                edited.fades.crossfade.length_s = length;
                if seam.room.length_reaching(f64::INFINITY) <= 0. {
                    "No trimmed audio to crossfade".to_string()
                } else {
                    format_length(length)
                }
            }
            FadePart::SeamCurve => {
                let curve = &mut edited.fades.crossfade.curve;
                *curve = dragged_curvature(*curve, local_y - drag.start_y, hit.body);
                format_curvature(*curve)
            }
        };
        self.state.resize_clip(edited.clone());
        drag.current = Some(edited);
        drag.label = Some(label);
    }

    /// Applies what `drag` did, as one undo step.
    pub(in crate::timeline) fn end_fade(&mut self, drag: FadeDrag) {
        if let Some(clip) = drag.current {
            self.state.commit_resize_clip(clip);
        }
    }

    /// The clip `id`'s fade at `end` (or its crossfade, with none) reset
    /// or given `shape`.
    pub(super) fn edit_fade_curve(
        &mut self,
        id: ClipId,
        end: Option<FadeEnd>,
        edit: impl FnOnce(&mut Fade, Fade),
    ) {
        let Some((_, clip)) = self.state.clip_with_track(id) else {
            return;
        };
        let mut edited = clip.clone();
        let defaults = ClipFades::default();
        match end {
            Some(end) => edit(end.fade_mut(&mut edited.fades), end.fade(&defaults)),
            None => edit(&mut edited.fades.crossfade, defaults.crossfade),
        }
        self.state.commit_resize_clip(edited);
    }
}
