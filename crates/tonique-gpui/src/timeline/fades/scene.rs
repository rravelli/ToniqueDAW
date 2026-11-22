//! What the scene shows of the fades over the clips: the crossfades, the
//! handles of the clip under the pointer (or the one dragged), the curve
//! under the pointer or dragged, and what a drag reads.

use crate::{
    app::Tonique,
    timeline::{
        clip::wave_ink,
        fades::{
            FadePart, body_of, clip_fade, lane_too_small,
            paint::{HotCurve, SceneFades, SceneSeam},
        },
        input::TimelineDrag,
        rows::Row,
    },
};

impl Tonique {
    pub(in crate::timeline) fn scene_fades(&self, rows: &[Row]) -> SceneFades {
        let left_behind = self.left_behind();
        let (hover, dragging) = match &self.timeline.drag {
            TimelineDrag::None => (self.timeline.hover, None),
            TimelineDrag::Fade(drag) => (None, Some(drag)),
            _ => (None, None),
        };
        let hover_hit = hover.and_then(|(local_x, local_y)| self.fade_at(rows, local_x, local_y));
        // The part under the pointer, or dragged, and its clip.
        let engaged = dragging
            .map(|drag| &drag.hit)
            .or(hover_hit.as_ref())
            .map(|hit| (hit.part, hit.clip.id));
        let bpm = self.state.bpm();
        let mut scene = SceneFades::default();
        for row in rows.iter().filter(|row| !row.is_group()) {
            let track = &row.track;
            let fades = self.track_fades(row, &left_behind);
            let (top, height) = (self.row_top(row), track.height);
            let ink = wave_ink(&self.theme, track.color);
            let handles_fit = !track.collapsed && !lane_too_small(height);
            for seam in &fades.seams {
                let Some(place) = self.seam_place(seam, &fades, top, height) else {
                    continue;
                };
                if place.crossing() {
                    scene.seams.push(SceneSeam {
                        lane: place.lane,
                        rising: place.rising,
                        falling: place.falling,
                        ink,
                    });
                }
                if !handles_fit {
                    continue;
                }
                let over =
                    hover.is_some_and(|(local_x, local_y)| place.area.contains(local_x, local_y));
                let held = dragging.is_some_and(|drag| {
                    drag.hit.clip.id == seam.later && drag.hit.part == FadePart::SeamHandle
                });
                if over || held {
                    scene.handles.push(place.square);
                }
                if place.crossing() && engaged == Some((FadePart::SeamCurve, seam.later)) {
                    for span in [place.rising, place.falling] {
                        scene.hot.push(HotCurve {
                            span,
                            body: place.lane,
                            ink,
                        });
                    }
                }
            }
            for clip in &fades.clips {
                let rect = self.clip_rect_at(clip, top, height);
                let crossed = fades.of(clip.id);
                let over = hover.is_some_and(|(local_x, local_y)| rect.contains(local_x, local_y));
                let held = engaged
                    .filter(|(_, id)| *id == clip.id)
                    .map(|(part, _)| part);
                if !over && held.is_none() {
                    continue;
                }
                for handle in self.clip_handles(clip, rect, crossed, track.collapsed) {
                    let shown = match &dragging {
                        Some(_) => held == Some(FadePart::Handle(handle.end)),
                        None => over,
                    };
                    if shown {
                        scene.handles.push(handle.square);
                    }
                    if handle.has_curve && held == Some(FadePart::Curve(handle.end)) {
                        let (_, span) = clip_fade(clip, rect, crossed, handle.end, bpm);
                        scene.hot.push(HotCurve {
                            span,
                            body: body_of(rect),
                            ink,
                        });
                    }
                }
            }
        }
        scene.label = dragging.and_then(|drag| {
            let text = drag.label.clone()?;
            let (position, _) = self.timeline.pointer?;
            let (local_x, local_y) = self.local(position);
            Some((local_x, local_y, text))
        });
        scene
    }
}
