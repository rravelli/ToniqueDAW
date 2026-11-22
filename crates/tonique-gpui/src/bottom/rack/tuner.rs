//! The tuner's editor, as the egui app's gauge: the note heard where it
//! sits in the chain, arrows towards where to tune it, a strip of segments
//! lit from the middle out to its cents, and a knob for the pitch of A4.
//! The egui app's history view isn't here yet.

use std::time::Instant;

use gpui::{AnyElement, Context, canvas, div, prelude::*, px};
use tonique_engine::{meter::ChannelMeter, pitch::PitchTracker};

use crate::{
    app::Tonique,
    bottom::{
        paint::{Align, circle, fill_rounded, polygon, text},
        rack::{
            editors::{Editor, EditorInput, KNOB_GAP},
            knob::KNOB_WIDTH,
        },
    },
    core::note::{IN_TUNE, Note, note},
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
const ROUNDING: f32 = 6.;

/// The pitch heard, followed frame after frame.
pub struct TunerState {
    tracker: PitchTracker,
    last: Option<Instant>,
}

impl TunerState {
    pub fn new() -> Self {
        Self {
            tracker: PitchTracker::new(),
            last: None,
        }
    }

    /// Takes in what `meter` heard since the last frame.
    pub fn update(&mut self, meter: &ChannelMeter, sample_rate: f32) {
        let now = Instant::now();
        let seconds = self
            .last
            .map_or(1. / 60., |last| (now - last).as_secs_f32().min(0.1));
        self.last = Some(now);
        self.tracker.update(meter, sample_rate, seconds);
    }
}

impl Tonique {
    pub(super) fn tuner_body(
        &self,
        input: &EditorInput,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let effect = input.effect;
        let enabled = input.enabled();
        let reference = effect.plugin.param("reference").map_or(440., |p| p.get());
        let editor = input.editor.clone();
        let paint_theme = theme.clone();
        let display = canvas(
            |_, _, _| {},
            move |bounds, _, window, context| {
                let theme = &paint_theme;
                let rect = Rect::of(bounds);
                fill_rounded(window, rect, ROUNDING, theme.bg_deep);
                let centre = (rect.left + rect.width / 2., rect.top + rect.height / 2.);
                if !enabled {
                    text(
                        window,
                        context,
                        "Bypassed",
                        centre,
                        Align::CENTER,
                        size::SMALL,
                        theme.text_muted,
                        None,
                    );
                    return;
                }
                let editor = editor.borrow();
                let Editor::Tuner(tuner) = &*editor else {
                    return;
                };
                let heard = tuner.tracker.hz().map(|hz| (hz, note(hz, reference)));
                paint_gauge(window, context, rect, heard, theme);
            },
        )
        .flex_1()
        .h_full();
        let knob = self.param_knob(
            effect,
            "reference",
            "A4",
            |hz| format!("{hz:.0} Hz"),
            false,
            theme,
            context,
        );
        div()
            .size_full()
            .flex()
            .items_center()
            .gap(px(KNOB_GAP))
            .child(display)
            .child(div().flex_none().w(px(KNOB_WIDTH)).children(knob))
            .into_any_element()
    }
}

/// The note, the strip and the readout, centred as one block in `rect`.
fn paint_gauge(
    window: &mut gpui::Window,
    context: &mut gpui::App,
    rect: Rect,
    heard: Option<(f32, Note)>,
    theme: &Theme,
) {
    let color = match heard {
        Some((_, heard)) if heard.in_tune() => theme.success,
        _ => theme.accent,
    };
    let note_height = NOTE_SIZE * 1.25;
    let height = note_height + NOTE_GAP + STRIP_HEIGHT + 8. + size::SMALL;
    let top = rect.top + (rect.height - height) / 2. + 6.;
    let centre_x = rect.left + rect.width / 2.;
    let note_centre = (centre_x, top + note_height / 2.);
    let strip_width = (rect.width - 24.).min(SEGMENTS as f32 * 12.);
    let strip = Rect::new(
        centre_x - strip_width / 2.,
        top + note_height + NOTE_GAP,
        strip_width,
        STRIP_HEIGHT,
    );
    paint_note(window, context, note_centre, heard.map(|(_, n)| n), theme);
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
    theme: &Theme,
) {
    let Some(heard) = heard else {
        text(
            window,
            context,
            "–",
            centre,
            Align::CENTER,
            NOTE_SIZE,
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
            let radius = GLOW_RADIUS - ring as f32 * GLOW_STEP;
            circle(window, centre, radius, with_alpha(theme.success, 6), None);
        }
    }
    let name_width = text(
        window,
        context,
        heard.name,
        centre,
        Align::CENTER,
        NOTE_SIZE,
        color,
        None,
    );
    text(
        window,
        context,
        heard.octave.to_string(),
        (
            centre.0 + name_width / 2. + 1.,
            centre.1 + NOTE_SIZE * 0.625 - 3.,
        ),
        Align::LEFT_BOTTOM,
        size::BODY,
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
    let flat = (centre.0 - ARROW_DISTANCE, centre.1);
    polygon(
        window,
        &[
            (flat.0 - ARROW_SIZE * 0.7, flat.1 - ARROW_SIZE),
            (flat.0 + ARROW_SIZE * 0.8, flat.1),
            (flat.0 - ARROW_SIZE * 0.7, flat.1 + ARROW_SIZE),
        ],
        lit(heard.cents < -IN_TUNE),
    );
    let sharp = (centre.0 + ARROW_DISTANCE, centre.1);
    polygon(
        window,
        &[
            (sharp.0 + ARROW_SIZE * 0.7, sharp.1 - ARROW_SIZE),
            (sharp.0 + ARROW_SIZE * 0.7, sharp.1 + ARROW_SIZE),
            (sharp.0 - ARROW_SIZE * 0.8, sharp.1),
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
