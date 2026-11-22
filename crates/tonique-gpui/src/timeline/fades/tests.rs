use tonique_engine::{edit::ClipId, time::BeatPos};

use super::*;
use crate::analysis::AudioInfo;

/// A 4 s file at 120 bpm: 8 beats, drawn 800 pixels wide below a header.
fn clip() -> (AudioClip, Rect) {
    let clip = AudioClip::new(ClipId(1), AudioInfo::silent(4.), BeatPos(0.), 120.);
    (clip, Rect::new(100., 40., 800., 80.))
}

#[test]
fn a_fade_spans_its_length_from_its_end() {
    let (mut clip, rect) = clip();
    clip.set_fade_in(1., Crossfades::default(), 120.);
    clip.set_fade_out(0.5, Crossfades::default(), 120.);
    let (_, fade_in) = clip_fade(&clip, rect, Crossfades::default(), FadeEnd::In, 120.);
    assert_eq!((fade_in.left, fade_in.right), (100., 300.));
    assert!(fade_in.rising);
    let (_, fade_out) = clip_fade(&clip, rect, Crossfades::default(), FadeEnd::Out, 120.);
    assert_eq!((fade_out.left, fade_out.right), (800., 900.));
    assert!(!fade_out.rising);
}

#[test]
fn a_curve_rises_from_the_bottom_of_the_body_to_its_top() {
    let (mut clip, rect) = clip();
    clip.set_fade_in(1., Crossfades::default(), 120.);
    let (_, span) = clip_fade(&clip, rect, Crossfades::default(), FadeEnd::In, 120.);
    let body = body_of(rect);
    let points = span.points(body, 0., 2000.);
    let (first, last) = (points[0], points[points.len() - 1]);
    assert_eq!(first, (100., body.bottom()));
    assert_eq!(last, (300., body.top));
    // Off screen, nothing is drawn.
    assert!(span.points(body, 400., 500.).is_empty());
    // Linear: half way up half way through, and grabbed there.
    let middle = (200., body.top + body.height / 2.);
    assert!(span.is_near(middle.0, middle.1, body, body));
    assert!(!span.is_near(middle.0, body.top, body, body));
}

#[test]
fn an_end_with_a_crossfade_shows_the_crossfade() {
    let (mut clip, rect) = clip();
    clip.set_fade_in(1., Crossfades::default(), 120.);
    let cross = Fade {
        length_s: 0.25,
        curve: FadeCurve::EQUAL_POWER,
    };
    let crossed = Crossfades {
        start_seam: true,
        start: Some(cross),
        ..Crossfades::default()
    };
    assert!(FadeEnd::In.crossed(crossed));
    assert!(!FadeEnd::Out.crossed(crossed));
    let (fade, span) = clip_fade(&clip, rect, crossed, FadeEnd::In, 120.);
    assert_eq!(fade, cross);
    assert_eq!(span.right - span.left, 50.);
}

#[test]
fn lengths_read_in_milliseconds_then_seconds() {
    assert_eq!(format_length(0.25), "250 ms");
    assert_eq!(format_length(1.5), "1.50 s");
}
