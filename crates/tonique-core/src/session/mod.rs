//! What the user is working on, never saved nor undone: the selected
//! tracks and clips, the clipboard, a trim under way, the rows' layout,
//! and the grid. The grid's zoom and scroll are the session's; its
//! [`grid::GridSettings`] are saved with the project, the grid computing
//! with them.

pub mod clipboard;
pub mod grid;
pub mod row_layout;
pub mod selection;

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
