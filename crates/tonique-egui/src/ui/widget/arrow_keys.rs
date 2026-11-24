//! The arrow keys for a value control: clicked (or dragged), it takes the
//! keyboard focus and the arrows step it, outlined in the accent; Esc or a
//! click elsewhere lets go. Other shortcuts still run while it has them.

use egui::{Context, EventFilter, Id, Key, Modifiers, Rect, Response, Stroke, StrokeKind, Ui};

use crate::ui::theme::ThemeExt;
use tonique_core::utils::steps::Presses;

/// Where the control holding the arrows says it is, each frame it does.
fn holder_id() -> Id {
    Id::new("arrow-keys-holder")
}

/// Takes the focus when `response` is clicked or dragged, and lets go on a
/// click elsewhere. The arrow keys pressed while it has it, consumed.
pub fn arrow_presses(ui: &Ui, response: &Response) -> Presses {
    if response.clicked() || response.drag_started() {
        response.request_focus();
    } else if response.has_focus() && response.clicked_elsewhere() {
        response.surrender_focus();
    }
    if !response.has_focus() {
        return Presses::default();
    }
    ui.ctx()
        .data_mut(|data| data.insert_temp(holder_id(), response.id));
    // Not to move the focus to another widget.
    ui.memory_mut(|memory| {
        memory.set_focus_lock_filter(
            response.id,
            EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                ..Default::default()
            },
        )
    });
    ui.input_mut(|input| {
        let mut presses = Presses::default();
        for (key, up) in [
            (Key::ArrowUp, true),
            (Key::ArrowRight, true),
            (Key::ArrowDown, false),
            (Key::ArrowLeft, false),
        ] {
            // Shifted first: without, a key matches with Shift too.
            while input.consume_key(Modifiers::SHIFT, key) {
                presses.press(up, true);
            }
            while input.consume_key(Modifiers::NONE, key) {
                presses.press(up, false);
            }
        }
        presses
    })
}

/// Whether the focused widget is a value control taking the arrow keys:
/// shortcuts other than the arrows still run then.
pub fn holds_arrows(context: &Context) -> bool {
    let holder: Option<Id> = context.data(|data| data.get_temp(holder_id()));
    holder.is_some() && context.memory(|memory| memory.focused()) == holder
}

/// Whether `key` with `modifiers` is one the holder takes.
pub fn is_arrow(key: Key, modifiers: Modifiers) -> bool {
    matches!(
        key,
        Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight
    ) && (modifiers.is_none() || modifiers == Modifiers::SHIFT)
}

/// The accent outline of a control holding the arrows, around `rect`.
pub fn paint_outline(ui: &Ui, rect: Rect, radius: f32) {
    let accent = ui.app_theme().accent;
    ui.painter()
        .rect_stroke(rect, radius, Stroke::new(1., accent), StrokeKind::Inside);
}
