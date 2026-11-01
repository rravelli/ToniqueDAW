//! The tuner's gauge: the note, arrows towards where to tune it, and a
//! strip of segments lit from the middle out to its cents.

use super::{IN_TUNE, Note};
use crate::ui::theme::{Theme, font_size};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

/// Segments across the strip, one every 4 cents from -48 to 48.
const SEGMENTS: i32 = 25;
const SEGMENT_CENTS: f32 = 4.;
const SEGMENT_WIDTH: f32 = 3.;
const STRIP_HEIGHT: f32 = 16.;
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

/// Paint the gauge in `rect` for the pitch `heard` (in Hz) and its note.
/// `large` in a full window.
pub fn paint(
    painter: &Painter,
    rect: Rect,
    heard: Option<(f32, Note)>,
    large: bool,
    theme: &Theme,
) {
    let scale = if large { 2. } else { 1. };
    let color = match heard {
        Some((_, n)) if n.in_tune() => theme.success,
        _ => theme.accent,
    };

    // The note, the strip and the readout, centred as one block.
    let note_height = font_size::DISPLAY * scale * 1.25;
    let strip_height = STRIP_HEIGHT * scale;
    let height = note_height + NOTE_GAP * scale + strip_height + 8. + font_size::SMALL;
    let top = rect.center().y - height / 2. + 6.;
    let note_center = pos2(rect.center().x, top + note_height / 2.);
    let strip = Rect::from_center_size(
        pos2(
            rect.center().x,
            top + note_height + NOTE_GAP * scale + strip_height / 2.,
        ),
        vec2(
            (rect.width() - 24.).min(SEGMENTS as f32 * 12. * scale),
            strip_height,
        ),
    );
    let readout_y = strip.bottom() + 8. + font_size::SMALL;

    paint_note(painter, note_center, heard.map(|(_, n)| n), scale, theme);
    paint_strip(painter, strip, heard.map(|(_, n)| n.cents), color, theme);

    let (text, text_color) = match heard {
        Some((hz, n)) => (
            format!("{:+.0} ct  ·  {hz:.1} Hz", n.cents),
            theme.text_muted,
        ),
        None => ("No pitch".into(), theme.text_disabled),
    };
    painter.text(
        pos2(rect.center().x, readout_y),
        Align2::CENTER_BOTTOM,
        text,
        FontId::proportional(font_size::SMALL),
        text_color,
    );
}

/// The note's name with its octave low at its right, a glow behind it
/// once in tune, and an arrow lit on the side it's off to.
fn paint_note(painter: &Painter, center: Pos2, note: Option<Note>, scale: f32, theme: &Theme) {
    let name_font = FontId::proportional(font_size::DISPLAY * scale);
    let Some(n) = note else {
        painter.text(
            center,
            Align2::CENTER_CENTER,
            "–",
            name_font,
            theme.text_disabled,
        );
        return;
    };
    let color = if n.in_tune() {
        theme.success
    } else {
        theme.text
    };
    if n.in_tune() {
        for ring in 0..GLOW_RINGS {
            let radius = GLOW_RADIUS - ring as f32 * GLOW_STEP;
            painter.circle_filled(center, radius * scale, theme.success.gamma_multiply(0.022));
        }
    }
    let name = painter.layout_no_wrap(n.name.into(), name_font, color);
    let octave = painter.layout_no_wrap(
        n.octave.to_string(),
        FontId::proportional(font_size::BODY * scale),
        color,
    );
    let width = name.size().x + 1. + octave.size().x;
    let left = center.x - width / 2.;
    let top = center.y - name.size().y / 2.;
    let octave_at = pos2(
        left + name.size().x + 1.,
        top + name.size().y - octave.size().y - 3. * scale,
    );
    painter.galley(octave_at, octave, color);
    painter.galley(pos2(left, top), name, color);

    // Pointing the way to tune: right (up) when flat, left when sharp.
    let off = |lit: bool| {
        if lit {
            theme.accent
        } else {
            theme.text_disabled.gamma_multiply(0.4)
        }
    };
    let (size, distance) = (ARROW_SIZE * scale, ARROW_DISTANCE * scale);
    let flat = pos2(center.x - distance, center.y);
    painter.add(Shape::convex_polygon(
        vec![
            flat + vec2(-size * 0.7, -size),
            flat + vec2(size * 0.8, 0.),
            flat + vec2(-size * 0.7, size),
        ],
        off(n.cents < -IN_TUNE),
        Stroke::NONE,
    ));
    let sharp = pos2(center.x + distance, center.y);
    painter.add(Shape::convex_polygon(
        vec![
            sharp + vec2(size * 0.7, -size),
            sharp + vec2(size * 0.7, size),
            sharp + vec2(-size * 0.8, 0.),
        ],
        off(n.cents > IN_TUNE),
        Stroke::NONE,
    ));
}

/// Segments from -48 to 48 cents, lit from the middle out to `cents`; the
/// one at it brightest and tallest.
fn paint_strip(painter: &Painter, strip: Rect, cents: Option<f32>, color: Color32, theme: &Theme) {
    let half = SEGMENTS / 2;
    let step = strip.width() / (SEGMENTS - 1) as f32;
    let nearest =
        cents.map(|c| (c / SEGMENT_CENTS).round().clamp(-half as f32, half as f32) as i32);
    for i in -half..=half {
        let x = strip.center().x + i as f32 * step;
        let at = nearest == Some(i);
        let lit = nearest.is_some_and(|n| (n.min(0)..=n.max(0)).contains(&i));
        let height = if at || i == 0 {
            strip.height()
        } else {
            strip.height() * 0.55
        };
        let fill = if at {
            color
        } else if lit {
            color.gamma_multiply(0.45)
        } else if i == 0 {
            theme.success.gamma_multiply(0.35)
        } else {
            theme.text_disabled.gamma_multiply(0.35)
        };
        let segment =
            Rect::from_center_size(pos2(x, strip.center().y), vec2(SEGMENT_WIDTH, height));
        if at {
            painter.rect_filled(segment.expand(2.), 3., color.gamma_multiply(0.2));
        }
        painter.rect_filled(segment, SEGMENT_WIDTH / 2., fill);
    }
}
