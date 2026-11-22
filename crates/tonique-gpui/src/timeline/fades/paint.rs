//! Painting fades: a clip's own over its waveform, crossfades over the
//! seams, then the handles, the curves lit and what a drag reads.

use gpui::Window;

use crate::{
    bottom::paint::{fill, fill_rounded, polygon, polyline},
    core::clip::{AudioClip, Crossfades},
    geometry::Rect,
    theme::{Color, Theme, hsla, mono, size, with_alpha},
    timeline::fades::{FadeEnd, FadeSpan, body_of, clip_fade},
    widgets::{paint_line, shape},
};

const SHADE_ALPHA: u8 = 130;
const CROSSFADE_SHADE_ALPHA: u8 = 70;
const CURVE_WIDTH: f32 = 1.;
const HOT_CURVE_WIDTH: f32 = 2.;
/// Between the pointer and what a drag reads, and around its text.
const LABEL_OFFSET: f32 = 14.;
const LABEL_PADDING: f32 = 4.;

/// A crossfade across a seam, local to the timeline.
pub struct SceneSeam {
    pub lane: Rect,
    pub rising: FadeSpan,
    pub falling: FadeSpan,
    pub ink: Color,
}

/// A curve drawn thicker, under the pointer or dragged: local to the
/// timeline, over `body`.
pub struct HotCurve {
    pub span: FadeSpan,
    pub body: Rect,
    pub ink: Color,
}

/// What the scene shows of the fades over the clips.
#[derive(Default)]
pub struct SceneFades {
    pub seams: Vec<SceneSeam>,
    /// The handles shown, local to the timeline.
    pub handles: Vec<Rect>,
    pub hot: Vec<HotCurve>,
    /// What a drag reads, beside the pointer (local to the timeline).
    pub label: Option<(f32, f32, String)>,
}

/// `span` moved by `(left, top)`: from the timeline into the window.
fn moved(span: FadeSpan, left: f32) -> FadeSpan {
    FadeSpan {
        left: span.left + left,
        right: span.right + left,
        ..span
    }
}

fn moved_rect(rect: Rect, (left, top): (f32, f32)) -> Rect {
    Rect::new(rect.left + left, rect.top + top, rect.width, rect.height)
}

/// Whatever the fade takes away over `body`: all that's above its curve.
fn shade(window: &mut Window, span: &FadeSpan, body: Rect, shown: Rect, color: Color) {
    let points = span.points(body, shown.left, shown.right());
    let (Some(&(first, _)), Some(&(last, _))) = (points.first(), points.last()) else {
        return;
    };
    let mut area = vec![(first, body.top)];
    area.extend(points);
    area.push((last, body.top));
    polygon(window, &area, color);
}

fn line(window: &mut Window, span: &FadeSpan, body: Rect, shown: Rect, width: f32, ink: Color) {
    let points = span.points(body, shown.left, shown.right());
    if points.len() > 1 {
        polyline(window, &points, width, ink);
    }
}

/// `clip`'s fades over its body, `rect` in the window, as far as `shown`.
pub fn paint_clip_fades(
    window: &mut Window,
    theme: &Theme,
    (clip, crossed, bpm): (&AudioClip, Crossfades, f32),
    rect: Rect,
    shown: Rect,
    ink: Color,
) {
    let body = body_of(rect);
    if body.height < 2. {
        return;
    }
    let shade_color = with_alpha(theme.bg_base, SHADE_ALPHA);
    for end in [FadeEnd::In, FadeEnd::Out] {
        let (fade, span) = clip_fade(clip, rect, crossed, end, bpm);
        if end.crossed(crossed) || fade.length_s <= 0. {
            continue;
        }
        shade(window, &span, body, shown, shade_color);
        line(window, &span, body, shown, CURVE_WIDTH, ink);
    }
}

/// The crossfades, over the clips they cross; `origin` is the timeline's
/// top left in the window, `lanes` what's shown of them.
pub fn paint_seams(
    window: &mut Window,
    fades: &SceneFades,
    theme: &Theme,
    origin: (f32, f32),
    lanes: Rect,
) {
    for seam in &fades.seams {
        let lane = moved_rect(seam.lane, origin);
        let shown = lane.intersect(lanes);
        if shown.is_empty() {
            continue;
        }
        fill(
            window,
            shown,
            with_alpha(theme.bg_base, CROSSFADE_SHADE_ALPHA),
        );
        for span in [seam.rising, seam.falling] {
            line(
                window,
                &moved(span, origin.0),
                lane,
                shown,
                CURVE_WIDTH,
                seam.ink,
            );
        }
    }
}

/// The curves lit and the handles, over the clips and crossfades.
pub fn paint_handles(
    window: &mut Window,
    fades: &SceneFades,
    theme: &Theme,
    origin: (f32, f32),
    lanes: Rect,
) {
    for hot in &fades.hot {
        let body = moved_rect(hot.body, origin);
        let shown = body.intersect(lanes);
        line(
            window,
            &moved(hot.span, origin.0),
            body,
            shown,
            HOT_CURVE_WIDTH,
            hot.ink,
        );
    }
    for square in &fades.handles {
        let square = moved_rect(*square, origin);
        let outline = Rect::new(
            square.left - 1.,
            square.top - 1.,
            square.width + 2.,
            square.height + 2.,
        );
        fill_rounded(window, outline, 2., theme.bg_base);
        fill_rounded(window, square, 1., theme.text);
    }
}

/// What a drag reads, in a box beside the pointer.
pub fn paint_label(
    window: &mut Window,
    context: &mut gpui::App,
    fades: &SceneFades,
    theme: &Theme,
    origin: (f32, f32),
) {
    let Some((label_x, label_y, text)) = &fades.label else {
        return;
    };
    let shaped = shape(window, text.clone(), size::SMALL, hsla(theme.text), mono());
    let width = f32::from(shaped.width) + 2. * LABEL_PADDING;
    let height = size::SMALL * 1.25 + LABEL_PADDING;
    let left = origin.0 + label_x + LABEL_OFFSET;
    let top = origin.1 + label_y + LABEL_OFFSET;
    let box_rect = Rect::new(left, top, width, height);
    fill_rounded(window, box_rect, 3., theme.bg_raised);
    paint_line(
        window,
        context,
        &shaped,
        left + LABEL_PADDING,
        top + LABEL_PADDING / 2.,
        size::SMALL,
    );
}
