//! Editor of [`EffectKind::Tuner`](crate::core::effect::EffectKind::Tuner):
//! the note heard where it sits in the chain, a needle for how many cents
//! off it is, and a knob for the pitch of A4.

use crate::ui::{
    effects::{EditorContext, EffectEdit, EffectEditor, track_drag},
    theme::{ThemeExt, font_size},
    widget::knob::Knob,
};
use egui::{Align, Align2, FontId, Layout, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use tonique_engine::pitch::PitchTracker;

/// Note names from C, sharps for the black keys.
const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
/// Off by at most this many cents, a note is in tune.
const IN_TUNE: f32 = 3.;
/// The scale's marks, in cents.
const MARKS: [f32; 5] = [-50., -25., 0., 25., 50.];
const SCALE_HEIGHT: f32 = 14.;
const GAP: f32 = 6.;

/// A pitch as a note: its name, octave (A4 is 440 Hz by default) and how
/// far off it is, from -50 to 50 cents.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub name: &'static str,
    pub octave: i32,
    pub cents: f32,
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

pub struct TunerEditor {
    tracker: PitchTracker,
}

impl TunerEditor {
    pub fn new() -> Self {
        Self {
            tracker: PitchTracker::new(),
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
        painter.rect_filled(display, 2., theme.bg_deep);
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
        self.tracker
            .update(&tap.output, cx.sample_rate, ui.input(|i| i.stable_dt));
        let heard = self
            .tracker
            .hz()
            .map(|hz| (hz, note(hz, reference.map_or(440., |p| p.get()))));

        let in_tune = heard.is_some_and(|(_, n)| n.cents.abs() <= IN_TUNE);
        let color = if in_tune { theme.success } else { theme.text };
        let scale_top = display.center().y + 6.;

        // The note, its octave low at its right.
        let name_font = FontId::proportional(font_size::DISPLAY);
        let note_center = pos2(
            display.center().x,
            scale_top - GAP - font_size::DISPLAY / 2.,
        );
        match heard {
            Some((_, n)) => {
                let name = painter.layout_no_wrap(n.name.into(), name_font, color);
                let octave = painter.layout_no_wrap(
                    n.octave.to_string(),
                    FontId::proportional(font_size::BODY),
                    color,
                );
                let width = name.size().x + 1. + octave.size().x;
                let left = note_center.x - width / 2.;
                let top = note_center.y - name.size().y / 2.;
                let baseline = top + name.size().y;
                painter.galley(
                    pos2(left + name.size().x + 1., baseline - octave.size().y - 3.),
                    octave,
                    color,
                );
                painter.galley(pos2(left, top), name, color);
            }
            None => {
                painter.text(
                    note_center,
                    Align2::CENTER_CENTER,
                    "–",
                    name_font,
                    theme.text_disabled,
                );
            }
        }

        // The scale, from 50 cents flat to 50 sharp.
        let scale = Rect::from_min_size(
            pos2(display.left() + 12., scale_top),
            vec2(display.width() - 24., SCALE_HEIGHT),
        );
        let x = |cents: f32| scale.left() + (cents + 50.) / 100. * scale.width();
        for mark in MARKS {
            let height = if mark == 0. {
                SCALE_HEIGHT
            } else {
                SCALE_HEIGHT / 2.
            };
            painter.vline(
                x(mark),
                (scale.center().y - height / 2.)..=(scale.center().y + height / 2.),
                Stroke::new(1., theme.text_disabled),
            );
        }
        painter.hline(
            scale.x_range(),
            scale.center().y,
            Stroke::new(1., theme.grid_beat),
        );
        let in_tune_band = Rect::from_x_y_ranges(x(-IN_TUNE)..=x(IN_TUNE), scale.y_range());
        painter.rect_filled(in_tune_band, 1., theme.success.gamma_multiply(0.25));
        if let Some((_, n)) = heard {
            let needle = if in_tune { theme.success } else { theme.accent };
            painter.vline(
                x(n.cents),
                scale.top() - 2.0..=scale.bottom() + 2.,
                Stroke::new(2., needle),
            );
        }

        let reading = match heard {
            Some((hz, n)) => format!("{:+.0} ct   {hz:.1} Hz", n.cents),
            None => "No pitch".into(),
        };
        painter.text(
            pos2(display.center().x, scale.bottom() + GAP),
            Align2::CENTER_TOP,
            reading,
            FontId::proportional(font_size::SMALL),
            if heard.is_some() {
                theme.text_muted
            } else {
                theme.text_disabled
            },
        );
    }

    fn width(&self) -> f32 {
        220.
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
