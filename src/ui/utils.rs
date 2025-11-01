use egui::Rect;

use crate::{
    core::{
        state::ToniqueProjectState,
        track::{DEFAULT_TRACK_HEIGHT, TrackReferenceCore},
    },
    ui::track::HANDLE_HEIGHT,
};

pub fn get_track_y(track_index: usize, viewport: Rect, state: &ToniqueProjectState) -> f32 {
    let mut y = viewport.top();
    let mut curr_index = 0;
    for track in state.tracks() {
        if curr_index == track_index {
            return y;
        }
        curr_index += 1;
        y += track.height + HANDLE_HEIGHT;
    }
    return (track_index - curr_index) as f32 * (DEFAULT_TRACK_HEIGHT + HANDLE_HEIGHT) + y;
}

pub fn find_track_at(
    state: &mut ToniqueProjectState,
    viewport: Rect,
    y_pos: f32,
) -> (Option<TrackReferenceCore>, f32) {
    let mut y = viewport.top();
    let tracks = state.tracks();

    if y_pos <= viewport.top()
        && let Some(track) = state.track_from_index(0)
    {
        return (Some(track), 0.);
    }

    for track in tracks {
        if y - state.grid.offset.y <= y_pos
            && y_pos <= y + track.height + HANDLE_HEIGHT - state.grid.offset.y
        {
            return (Some(track), y);
        }
        y += track.height + HANDLE_HEIGHT;
    }
    (None, y)
}
