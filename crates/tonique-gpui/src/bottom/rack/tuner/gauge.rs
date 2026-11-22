//! The tuner's gauge: the note, arrows towards where to tune it, and a
//! strip of segments lit from the middle out to its cents.

use crate::{
    bottom::paint::{Align, circle, fill_rounded, polygon, text},
    core::note::{IN_TUNE, Note},
    geometry::Rect,
    theme::{Color, Theme, size, with_alpha},
};

/// Segments across the strip, one every 4 cents from -48 to 48.
const SEGMENTS: i32 = 25;
const SEGMENT_CENTS: f32 = 4.;
const SEGMENT_WIDTH: f32 = 3.;
const STRIP_HEIGHT: f32 = 16.;
/// Size of the note's name.
const NOTE_SIZE: f32 = 28.;
/// From the note's centre to an arrow's.
const ARROW_DISTANCE: f32 = 38.;
const ARROW_SIZE: f32 = 6.;
/// Between the note and the strip.
const NOTE_GAP: f32 = 10.;
/// The glow behind a note in tune: its radius, and rings of it stacked
/// each this much less wide, so it fades out smoothly.
const GLOW_RADIUS: f32 = 34.;
const GLOW_RINGS: usize = 10;
const GLOW_STEP: f32 = 1.8;

/// The note, the strip and the readout, centred as one block in `rect`.
pub fn paint(
    window: &mut gpui::Window,
    context: &mut gpui::App,
    rect: Rect,
    heard: Option<(f32, Note)>,
    large: bool,
    theme: &Theme,
) {
    let scale = if large { 2. } else { 1. };
    let color = match heard {
        Some((_, heard)) if heard.in_tune() => theme.success,
        _ => theme.accent,
    };
    let note_height = NOTE_SIZE * scale * 1.25;
    let strip_height = STRIP_HEIGHT * scale;
    let height = note_height + NOTE_GAP * scale + strip_height + 8. + size::SMALL;
    let top = rect.top + (rect.height - height) / 2. + 6.;
    let centre_x = rect.left + rect.width / 2.;
    let note_centre = (centre_x, top + note_height / 2.);
    let strip_width = (rect.width - 24.).min(SEGMENTS as f32 * 12. * scale);
    let strip = Rect::new(
        centre_x - strip_width / 2.,
        top + note_height + NOTE_GAP * scale,
        strip_width,
        strip_height,
    );
    paint_note(
        window,
        context,
        note_centre,
        heard.map(|(_, n)| n),
        scale,
        theme,
    );
    paint_strip(window, strip, heard.map(|(_, n)| n.cents), color, theme);
    let (readout, readout_color) = match heard {
        Some((hz, heard)) => (
            format!("{:+.0} ct  ·  {hz:.1} Hz", heard.cents),
            theme.text_muted,
        ),
        None => ("No pitch".into(), theme.text_disabled),
    };
    text(
        window,
        context,
        readout,
        (centre_x, strip.bottom() + 8. + size::SMALL),
        Align::CENTER_BOTTOM,
        size::SMALL,
        readout_color,
        None,
    );
}

/// The note's name with its octave at its right, a glow behind it once in
/// tune, and an arrow lit on the side it's off to.
fn paint_note(
    window: &mut gpui::Window,
    context: &mut gpui::App,
    centre: (f32, f32),
    heard: Option<Note>,
    scale: f32,
    theme: &Theme,
) {
    let note_size = NOTE_SIZE * scale;
    let Some(heard) = heard else {
        text(
            window,
            context,
            "–",
            centre,
            Align::CENTER,
            note_size,
            theme.text_disabled,
            None,
        );
        return;
    };
    let color = if heard.in_tune() {
        theme.success
    } else {
        theme.text
    };
    if heard.in_tune() {
        for ring in 0..GLOW_RINGS {
            let radius = (GLOW_RADIUS - ring as f32 * GLOW_STEP) * scale;
            circle(window, centre, radius, with_alpha(theme.success, 6), None);
        }
    }
    let name_width = text(
        window,
        context,
        heard.name,
        centre,
        Align::CENTER,
        note_size,
        color,
        None,
    );
    text(
        window,
        context,
        heard.octave.to_string(),
        (
            centre.0 + name_width / 2. + 1.,
            centre.1 + note_size * 0.625 - 3. * scale,
        ),
        Align::LEFT_BOTTOM,
        size::BODY * scale,
        color,
        None,
    );

    // Pointing the way to tune: right (up) when flat, left when sharp.
    let lit = |on: bool| {
        if on {
            theme.accent
        } else {
            with_alpha(theme.text_disabled, 102)
        }
    };
    let (arrow, distance) = (ARROW_SIZE * scale, ARROW_DISTANCE * scale);
    let flat = (centre.0 - distance, centre.1);
    polygon(
        window,
        &[
            (flat.0 - arrow * 0.7, flat.1 - arrow),
            (flat.0 + arrow * 0.8, flat.1),
            (flat.0 - arrow * 0.7, flat.1 + arrow),
        ],
        lit(heard.cents < -IN_TUNE),
    );
    let sharp = (centre.0 + distance, centre.1);
    polygon(
        window,
        &[
            (sharp.0 + arrow * 0.7, sharp.1 - arrow),
            (sharp.0 + arrow * 0.7, sharp.1 + arrow),
            (sharp.0 - arrow * 0.8, sharp.1),
        ],
        lit(heard.cents > IN_TUNE),
    );
}

/// Segments from -48 to 48 cents, lit from the middle out to `cents`; the
/// one at it brightest and tallest.
fn paint_strip(
    window: &mut gpui::Window,
    strip: Rect,
    cents: Option<f32>,
    color: Color,
    theme: &Theme,
) {
    let half = SEGMENTS / 2;
    let step = strip.width / (SEGMENTS - 1) as f32;
    let centre = (strip.left + strip.width / 2., strip.top + strip.height / 2.);
    let nearest = cents.map(|cents| {
        (cents / SEGMENT_CENTS)
            .round()
            .clamp(-half as f32, half as f32) as i32
    });
    for segment in -half..=half {
        let x = centre.0 + segment as f32 * step;
        let at = nearest == Some(segment);
        let lit =
            nearest.is_some_and(|nearest| (nearest.min(0)..=nearest.max(0)).contains(&segment));
        let height = if at || segment == 0 {
            strip.height
        } else {
            strip.height * 0.55
        };
        let fill = if at {
            color
        } else if lit {
            with_alpha(color, 115)
        } else if segment == 0 {
            with_alpha(theme.success, 90)
        } else {
            with_alpha(theme.text_disabled, 90)
        };
        let rect = Rect::new(
            x - SEGMENT_WIDTH / 2.,
            centre.1 - height / 2.,
            SEGMENT_WIDTH,
            height,
        );
        if at {
            let glow = Rect::new(
                rect.left - 2.,
                rect.top - 2.,
                rect.width + 4.,
                rect.height + 4.,
            );
            fill_rounded(window, glow, 3., with_alpha(color, 51));
        }
        fill_rounded(window, rect, SEGMENT_WIDTH / 2., fill);
    }
}
