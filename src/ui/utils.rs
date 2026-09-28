use egui::Rect;

use crate::{
    core::{
        state::ToniqueProjectState,
        track::{DEFAULT_TRACK_HEIGHT, TrackKind, TrackReferenceCore},
    },
    ui::track::HANDLE_HEIGHT,
};

/// Top of the lane of track `track_index` (in the engine's order). A track
/// inside a folded group is at its group's row.
pub fn track_y(track_index: usize, viewport: Rect, state: &ToniqueProjectState) -> f32 {
    let mut y = viewport.top();
    let mut last = None;
    for row in state.rows() {
        if row.index > track_index {
            break;
        }
        last = Some(y);
        if row.kind == TrackKind::Audio && row.index == track_index {
            return y;
        }
        y += row.height + HANDLE_HEIGHT;
    }
    match last {
        Some(top) if track_index < state.track_count() => top,
        _ => {
            (track_index.saturating_sub(state.track_count())) as f32
                * (DEFAULT_TRACK_HEIGHT + HANDLE_HEIGHT)
                + y
        }
    }
}

/// The track under `y_pos` and the top of its row. Over a group's row, the
/// group's first track and its own row (the group's, when folded): clips
/// always land, and are previewed, on tracks.
pub fn find_track_at(
    state: &mut ToniqueProjectState,
    viewport: Rect,
    y_pos: f32,
) -> (Option<TrackReferenceCore>, f32) {
    let mut y = viewport.top();

    if y_pos <= viewport.top()
        && let Some(track) = state.track_from_index(0)
    {
        return (Some(track), 0.);
    }

    for row in state.rows() {
        if y - state.grid.offset.y <= y_pos
            && y_pos <= y + row.height + HANDLE_HEIGHT - state.grid.offset.y
        {
            return match row.kind {
                TrackKind::Audio => (Some(row), y),
                TrackKind::Group => (
                    state.track_from_index(row.index),
                    track_y(row.index, viewport, state),
                ),
            };
        }
        y += row.height + HANDLE_HEIGHT;
    }
    (None, y)
}
