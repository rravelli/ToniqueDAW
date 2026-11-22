//! Editor of [`EffectKind::Tuner`](crate::core::effect::EffectKind::Tuner):
//! the note heard where it sits in the chain, either on a gauge of how
//! many cents off it is or as a trace of the last seconds, and a knob for
//! the pitch of A4.

mod gauge;
mod history;

pub use crate::core::note::{IN_TUNE, NAMES, Note, note};
use crate::ui::{
    effects::{EditorContext, EffectEdit, EffectEditor, icon_button, track_drag},
    theme::{ThemeExt, font_size},
    widget::knob::Knob,
};
use egui::{Align, Align2, FontId, Layout, Rect, Sense, Ui, UiBuilder, pos2, vec2};
use egui_phosphor::regular::{CHART_LINE, GAUGE};
use history::History;
use tonique_engine::pitch::PitchTracker;

/// Between the display and the knob.
const GAP: f32 = 6.;
/// Corner radius of the display.
const ROUNDING: f32 = 6.;
/// Inside the display, around what it shows.
const PADDING: f32 = 4.;
/// The view buttons, at the display's top left.
const BUTTON: f32 = 15.;

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
        self.history.follow(reference, dt);

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
