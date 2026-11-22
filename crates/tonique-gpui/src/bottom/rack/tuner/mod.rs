//! The tuner's editor, as the egui app's: the note heard where it sits in
//! the chain, either on a gauge of how many cents off it is ([`gauge`]) or
//! as a trace of the last seconds ([`history`]), and a knob for the pitch
//! of A4.

mod gauge;
mod history;

use std::time::Instant;

use gpui::{AnyElement, Context, canvas, div, prelude::*, px};
use history::History;
use tonique_engine::{meter::ChannelMeter, pitch::PitchTracker};

use crate::{
    app::Tonique,
    bottom::{
        paint::{Align, fill_rounded, text},
        rack::{
            editors::{Editor, EditorInput, KNOB_GAP},
            knob::KNOB_WIDTH,
        },
    },
    core::note::note,
    geometry::Rect,
    icons,
    theme::{Theme, size},
    widgets::{Flat, glyph, tooltip},
};

/// Corner radius of the display.
const ROUNDING: f32 = 6.;
/// Inside the display, around what it shows.
const PADDING: f32 = 4.;
/// The view buttons, at the display's top left.
const BUTTON: f32 = 15.;
const BUTTON_ICON: f32 = 10.;

/// What the display shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    /// The note now, and a gauge of its cents.
    Gauge,
    /// The cents of the notes over the last seconds.
    History,
}

/// The pitch heard, followed frame after frame.
pub struct TunerState {
    tracker: PitchTracker,
    view: View,
    history: History,
    /// When it was made: the history's times count from it.
    start: Instant,
    last: Option<Instant>,
}

impl TunerState {
    pub fn new() -> Self {
        Self {
            tracker: PitchTracker::new(),
            view: View::Gauge,
            history: History::default(),
            start: Instant::now(),
            last: None,
        }
    }

    /// Seconds since it was made.
    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    /// Takes in what `meter` heard since the last frame, A4 being at
    /// `reference` Hz.
    pub fn update(&mut self, meter: &ChannelMeter, sample_rate: f32, reference: f32) {
        let now = Instant::now();
        let seconds = self
            .last
            .map_or(1. / 60., |last| (now - last).as_secs_f32().min(0.1));
        self.last = Some(now);
        self.tracker.update(meter, sample_rate, seconds);
        let time = self.now();
        self.history.push(time, self.tracker.hz());
        self.history.follow(reference, seconds);
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
        let large = input.expanded;
        let reference = effect
            .plugin
            .param("reference")
            .map_or(440., |param| param.get());
        let view = match &*input.editor.borrow() {
            Editor::Tuner(tuner) => tuner.view,
            _ => View::Gauge,
        };
        let editor = input.editor.clone();
        let paint_theme = theme.clone();
        let painting = canvas(
            |_, _, _| {},
            move |bounds, _, window, context| {
                let theme = &paint_theme;
                let rect = Rect::of(bounds);
                fill_rounded(window, rect, ROUNDING, theme.bg_deep);
                if !enabled {
                    text(
                        window,
                        context,
                        "Bypassed",
                        (rect.left + rect.width / 2., rect.top + rect.height / 2.),
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
                let inner = Rect::new(
                    rect.left + PADDING,
                    rect.top + PADDING,
                    rect.width - 2. * PADDING,
                    rect.height - 2. * PADDING,
                );
                match tuner.view {
                    View::Gauge => {
                        let heard = tuner.tracker.hz().map(|hz| (hz, note(hz, reference)));
                        gauge::paint(window, context, inner, heard, large, theme);
                    }
                    View::History => history::paint(
                        window,
                        context,
                        inner,
                        &tuner.history,
                        (tuner.now(), reference),
                        theme,
                    ),
                }
            },
        )
        .absolute()
        .size_full();
        let buttons = [
            (View::Gauge, icons::GAUGE, "Gauge"),
            (View::History, icons::CHART_LINE, "History"),
        ]
        .map(|(option, glyph_name, name)| {
            let editor = input.editor.clone();
            Flat::ghost(name, BUTTON)
                .selected(view == option)
                .show(
                    glyph(glyph_name, BUTTON_ICON, false),
                    theme,
                    context,
                    move |_, _| {
                        if let Editor::Tuner(tuner) = &mut *editor.borrow_mut() {
                            tuner.view = option;
                        }
                    },
                )
                .tooltip(tooltip(name, theme))
        });
        let display = div()
            .relative()
            .flex_1()
            .h_full()
            .child(painting)
            .when(enabled, |display| {
                display.child(
                    div()
                        .absolute()
                        .top(px(PADDING))
                        .left(px(PADDING))
                        .flex()
                        .gap(px(2.))
                        .children(buttons),
                )
            });
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
