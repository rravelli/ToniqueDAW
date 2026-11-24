//! What the user is working on, never saved nor undone: the selected
//! tracks and clips, the clipboard, a trim under way.

use tonique_engine::edit::TrackId;

use super::{clipboard::Clipboard, selection::ClipSelection};
use crate::arrangement::clip::AudioClip;

#[derive(Default)]
pub(super) struct Session {
    pub(super) selected_tracks: Vec<TrackId>,
    pub(super) clip_selection: ClipSelection,
    pub(super) clipboard: Clipboard,
    /// A clip being trimmed, as it will be once committed: shown instead
    /// of the edit's (see [`super::ProjectState::shown_clip`]).
    pub(super) resized_clip: Option<AudioClip>,
}
