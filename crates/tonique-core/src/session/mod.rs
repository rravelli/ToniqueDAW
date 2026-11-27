//! What the user is working on, never saved nor undone: the selected
//! tracks and clips, the clipboard, a trim under way ([`Session`]); and
//! the rules the project's views share: the rows' layout, the musical
//! [`grid::Grid`] (its settings are saved with the project) and the
//! [`timeline_view::TimelineView`] each view zooms and scrolls on its own.

pub mod clipboard;
pub mod grid;
pub mod row_layout;
pub mod selection;
pub mod timeline_view;

use tonique_engine::edit::TrackId;

use crate::document::clip::AudioClip;
use clipboard::Clipboard;
use selection::ClipSelection;

#[derive(Default)]
pub(crate) struct Session {
    pub(crate) selected_tracks: Vec<TrackId>,
    pub(crate) clip_selection: ClipSelection,
    pub(crate) clipboard: Clipboard,
    /// A clip being trimmed, as it will be once committed: shown instead
    /// of the edit's (see [`crate::project::ProjectState::shown_clip`]).
    pub(crate) resized_clip: Option<AudioClip>,
}
