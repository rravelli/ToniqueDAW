//! The arrow keys for a value control, as the egui app's `arrow_keys`: a
//! clicked control holds them (`app::arrows::hold`), each press steps it,
//! and it's outlined in the accent while it does.

use gpui::{Keystroke, Window, px};

use crate::{
    geometry::Rect,
    theme::{Color, hsla},
    utils::steps::Presses,
};

/// A press of an arrow key, alone or with Shift (a big step): up and right
/// step up, down and left back.
pub fn arrow_press(keystroke: &Keystroke) -> Option<Presses> {
    let modifiers = &keystroke.modifiers;
    if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
        return None;
    }
    let up = match keystroke.key.as_str() {
        "up" | "right" => true,
        "down" | "left" => false,
        _ => return None,
    };
    Some(Presses::one(up, modifiers.shift))
}

/// The outline of a control holding the arrow keys, around `rect`.
pub fn paint_outline(window: &mut Window, rect: Rect, radius: f32, accent: Color) {
    window.paint_quad(
        gpui::fill(rect.bounds(), gpui::transparent_black())
            .corner_radii(px(radius))
            .border_widths(px(1.))
            .border_color(hsla(accent)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_step_and_shift_makes_them_big() {
        let press = |text: &'static str| arrow_press(&Keystroke::parse(text).unwrap());
        assert_eq!(press("up"), Some(Presses::one(true, false)));
        assert_eq!(press("left"), Some(Presses::one(false, false)));
        assert_eq!(press("shift-right"), Some(Presses::one(true, true)));
        assert_eq!(press("ctrl-up"), None);
        assert_eq!(press("a"), None);
    }
}
