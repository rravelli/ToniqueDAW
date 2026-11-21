//! Helpers for egui's text fields.

use egui::{
    Context, Id,
    text::{CCursor, CCursorRange},
    text_edit::TextEditState,
};

/// Select all of `text` in the field `id`, so typing replaces it.
pub fn select_all(ctx: &Context, id: Id, text: &str) {
    let mut state = TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::two(
        CCursor::new(0),
        CCursor::new(text.chars().count()),
    )));
    state.store(ctx, id);
}
