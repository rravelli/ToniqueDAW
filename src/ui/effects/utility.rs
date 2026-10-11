//! Editor of [`EffectKind::Utility`](crate::core::effect::EffectKind::Utility):
//! gain, balance and width knobs, and mono and phase switches. Its level
//! shows on the meter every effect has.

use crate::ui::{
    effects::{EditorContext, EffectEditor, TOGGLE_SIZE},
    widget::knob::Knob,
};
use egui::{Align, Layout, Rect, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use tonique_engine::nodes::UTILITY_SILENT_DB;

const KNOB_GAP: f32 = 6.;
const TOGGLE_GAP: f32 = 2.;
/// Between the knobs and the switches.
const ROW_GAP: f32 = 6.;

pub struct UtilityEditor;

pub fn format_gain(db: f32) -> String {
    if db <= UTILITY_SILENT_DB {
        "-inf dB".into()
    } else {
        format!("{db:+.1} dB")
    }
}

/// `C` in the middle, else how far to a side, from 1 to 50 as Ableton does.
pub fn format_balance(balance: f32) -> String {
    let amount = (balance.abs() * 50.).round();
    if amount == 0. {
        "C".into()
    } else if balance < 0. {
        format!("{amount:.0}L")
    } else {
        format!("{amount:.0}R")
    }
}

impl EffectEditor for UtilityEditor {
    fn ui(&mut self, ui: &mut Ui, cx: &mut EditorContext) {
        // The controls centred.
        let (controls, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        let knobs = vec2(3. * Knob::WIDTH + 2. * KNOB_GAP, Knob::HEIGHT);
        let toggles = vec2(3. * TOGGLE_SIZE.x + 2. * TOGGLE_GAP, TOGGLE_SIZE.y);
        let top = controls.center().y - (knobs.y + ROW_GAP + toggles.y) / 2.;
        let row = |size: Vec2, top: f32| {
            Rect::from_min_size(pos2(controls.center().x - size.x / 2., top), size)
        };
        let mut knobs_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(row(knobs, top))
                .layout(Layout::left_to_right(Align::Min)),
        );
        knobs_ui.spacing_mut().item_spacing.x = KNOB_GAP;
        cx.param_knob(&mut knobs_ui, "gain", "Gain", &format_gain, false);
        cx.param_knob(&mut knobs_ui, "balance", "Balance", &format_balance, false);
        cx.param_knob(
            &mut knobs_ui,
            "width",
            "Width",
            &|w| format!("{:.0}%", w * 100.),
            false,
        );
        let mut toggles_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(row(toggles, top + knobs.y + ROW_GAP))
                .layout(Layout::left_to_right(Align::Min)),
        );
        toggles_ui.spacing_mut().item_spacing.x = TOGGLE_GAP;
        cx.param_toggle(
            &mut toggles_ui,
            "mono",
            "Mono",
            "Both sides the same: no width",
        );
        cx.param_toggle(
            &mut toggles_ui,
            "invert_left",
            "Ø L",
            "Invert the left's phase",
        );
        cx.param_toggle(
            &mut toggles_ui,
            "invert_right",
            "Ø R",
            "Invert the right's phase",
        );
    }

    fn width(&self) -> f32 {
        172.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_as_in_other_daws() {
        assert_eq!(format_gain(UTILITY_SILENT_DB), "-inf dB");
        assert_eq!(format_gain(-6.), "-6.0 dB");
        assert_eq!(format_gain(3.), "+3.0 dB");
        assert_eq!(format_balance(0.001), "C");
        assert_eq!(format_balance(-1.), "50L");
        assert_eq!(format_balance(0.3), "15R");
    }
}
