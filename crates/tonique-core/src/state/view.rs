//! How the document looks: saved with it, but not undoable (folding a
//! track or an effect isn't an edit).

use std::collections::{HashMap, HashSet};

use tonique_engine::edit::{PluginId, TrackId};

use super::MASTER_TRACK_ID;
use crate::arrangement::track::TrackView;

pub(super) struct DocumentView {
    /// Each track's look (name shown, colour, height, folded); kept for
    /// deleted tracks so undo restores them.
    pub(super) tracks: HashMap<TrackId, TrackView>,
    /// Effects shown folded.
    pub(super) collapsed_effects: HashSet<PluginId>,
}

impl DocumentView {
    /// A new project's: the master alone.
    pub(super) fn new() -> Self {
        let mut master = TrackView::new();
        master.name = "Master".into();
        Self {
            tracks: HashMap::from([(MASTER_TRACK_ID, master)]),
            collapsed_effects: HashSet::new(),
        }
    }
}
