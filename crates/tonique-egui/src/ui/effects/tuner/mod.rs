//! Editor of [`EffectKind::Tuner`](crate::core::effect::EffectKind::Tuner):
//! the note heard where it sits in the chain, either on a gauge of how
//! many cents off it is or as a trace of the last seconds, and a knob for
//! the pitch of A4.

mod gauge;
mod history;

use crate::ui::{
    effects::{EditorContext, EffectEdit, EffectEditor, icon_button, track_drag},
    theme::{ThemeExt, font_size},
    widget::knob::Knob,
};
use egui::{Align, Align2, FontId, Layout, Rect, Sense, Ui, UiBuilder, pos2, vec2};
use egui_phosphor::regular::{CHART_LINE, GAUGE};
use history::History;
use tonique_engine::pitch::PitchTracker;

/// Note names from C, sharps for the black keys.
const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
/// Off by at most this many cents, a note is in tune.
const IN_TUNE: f32 = 3.;
/// Between the display and the knob.
const GAP: f32 = 6.;
/// Corner radius of the display.
const ROUNDING: f32 = 6.;
/// Inside the display, around what it shows.
const PADDING: f32 = 4.;
/// The view buttons, at the display's top left.
const BUTTON: f32 = 15.;

/// A pitch as a note: its name, octave (A4 is 440 Hz by default) and how
/// far off it is, from -50 to 50 cents.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub name: &'static str,
    pub octave: i32,
    pub cents: f32,
}

impl Note {
    pub fn in_tune(&self) -> bool {
        self.cents.abs() <= IN_TUNE
    }

    /// The same note, whatever its tuning.
    fn same(&self, other: &Note) -> bool {
        (self.name, self.octave) == (other.name, other.octave)
    }
}

/// The note nearest `hz`, with A4 at `reference` Hz.
pub fn note(hz: f32, reference: f32) -> Note {
    let semis = 69. + 12. * (hz / reference).log2();
    let nearest = semis.round();
    let midi = nearest as i32;
    Note {
        name: NAMES[midi.rem_euclid(12) as usize],
        octave: midi.div_euclid(12) - 1,
        cents: (semis - nearest) * 100.,
    }
}

/// What the display shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    /// The note now, and a gauge of its cents.
    Gauge,
    /// The cents of the notes over the last seconds.
    History,
}

pub struct TunerEditor {
    tracker: PitchTracker,
    view: View,
    history: History,
}

impl TunerEditor {
    pub fn new() -> Self {
        Self {
            tracker: PitchTracker::new(),
            view: View::Gauge,
            history: History::default(),
        }
    }
}

impl EffectEditor for TunerEditor {
    fn ui(&mut self, ui: &mut Ui, cx: &mut EditorContext) {
        let area = ui.available_rect_before_wrap();
        let knob = Rect::from_center_size(
            pos2(area.right() - Knob::WIDTH / 2., area.center().y),
            vec2(Knob::WIDTH, Knob::HEIGHT),
        );
        let display = Rect::from_min_max(area.min, pos2(knob.left() - GAP, area.bottom()));
        ui.allocate_rect(area, Sense::hover());

        let effect = cx.effect;
        let reference = effect.plugin.param("reference");
        if let Some(p) = reference {
            let mut knob_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(knob)
                    .layout(Layout::left_to_right(Align::Min)),
            );
            let (id, before) = (p.id, p.get());
            let mut value = before;
            let response = knob_ui.add(
                Knob::new(&mut value, p.min..=p.max)
                    .default(Some(p.default))
                    .name("A4")
                    .format(&|hz| format!("{hz:.0} Hz"))
                    .active(cx.enabled()),
            );
            if response.changed() {
                p.set(value.round());
            }
            if let Some(old) = track_drag(&knob_ui, &response, before) {
                cx.edits.push(EffectEdit::Param {
                    id,
                    old,
                    new: p.get(),
                });
            }
        }

        let theme = ui.app_theme();
        let painter = ui.painter_at(display);
        painter.rect_filled(display, ROUNDING, theme.bg_deep);
        if !cx.enabled() {
            painter.text(
                display.center(),
                Align2::CENTER_CENTER,
                "Bypassed",
                FontId::proportional(font_size::SMALL),
                theme.text_muted,
            );
            return;
        }
        let tap = &effect.plugin.tap;
        // Keep it recording for half a second past the last frame shown.
        tap.watch_output_for((cx.sample_rate / 2.) as u32);
        let (dt, now) = ui.input(|i| (i.stable_dt, i.time));
        self.tracker.update(&tap.output, cx.sample_rate, dt);
        self.history.push(now, self.tracker.hz());

        let reference = reference.map_or(440., |p| p.get());
        let inner = display.shrink(PADDING);
        match self.view {
            View::Gauge => {
                let heard = self.tracker.hz().map(|hz| (hz, note(hz, reference)));
                gauge::paint(&painter, inner, heard, cx.expanded, &theme);
            }
            View::History => {
                history::paint(&painter, inner, &self.history, now, reference, &theme);
            }
        }

        // Over the display, out of the layout.
        let mut at = inner.left_top();
        for (view, icon, name) in [
            (View::Gauge, GAUGE, "Gauge"),
            (View::History, CHART_LINE, "History"),
        ] {
            let rect = Rect::from_min_size(at, vec2(BUTTON, BUTTON));
            let button = icon_button(icon).selected(self.view == view).tooltip(name);
            if ui.put(rect, button).clicked() {
                self.view = view;
            }
            at.x += BUTTON + 2.;
        }
    }

    fn width(&self) -> f32 {
        236.
    }

    fn expandable(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitches_read_as_notes() {
        assert_eq!(
            note(440., 440.),
            Note {
                name: "A",
                octave: 4,
                cents: 0.
            }
        );
        let e2 = note(82.41, 440.);
        assert_eq!((e2.name, e2.octave), ("E", 2));
        assert!(e2.cents.abs() < 0.1);
        // B3 is just under C4: the octave turns over at C.
        assert_eq!(note(246.94, 440.).octave, 3);
        assert_eq!(note(261.63, 440.).octave, 4);
        // 10 cents sharp, and flat.
        let sharp = note(440. * (10f32 / 1200.).exp2(), 440.);
        assert_eq!(sharp.name, "A");
        assert!((sharp.cents - 10.).abs() < 0.01);
        let flat = note(440. * (-10f32 / 1200.).exp2(), 440.);
        assert!((flat.cents + 10.).abs() < 0.01);
        // Over 50 cents sharp is the next note, flat.
        assert_eq!(note(440. * (60f32 / 1200.).exp2(), 440.).name, "A#");
    }

    #[test]
    fn the_reference_moves_every_note() {
        let a = note(432., 432.);
        assert_eq!((a.name, a.octave), ("A", 4));
        assert!(a.cents.abs() < 0.01);
        assert!(note(440., 432.).cents > 30.);
    }
}
