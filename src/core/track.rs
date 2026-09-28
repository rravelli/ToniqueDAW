use crate::core::clip::ClipCore;
use egui::Color32;
use tonique_engine::edit::TrackId;

#[derive(Debug, Clone)]
pub enum TrackSoloState {
    Soloing,
    NotSoloing,
    Solo,
}

/// What a row of the track list is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Audio,
    /// A bus holding tracks and groups.
    Group,
}

pub const DEFAULT_TRACK_HEIGHT: f32 = 60.;
pub const TRACK_COLLAPSED_HEIGHT: f32 = 22.;

/// Read-only snapshot of a track for the UI, built from the engine's edit.
#[derive(Debug, Clone)]
pub struct TrackReferenceCore {
    pub id: TrackId,
    pub clips: Vec<ClipCore>,
    pub muted: bool,
    pub volume: f32,
    pub arm: bool,
    pub name: String,
    pub height: f32,
    pub collapsed: bool,
    pub color: Color32,
    pub selected: bool,
    pub solo: TrackSoloState,
    /// Position in the engine's track list; for a group, its first track's.
    pub index: usize,
    pub kind: TrackKind,
    /// Groups it's nested in.
    pub depth: usize,
}

impl TrackReferenceCore {
    pub fn disabled(&self) -> bool {
        self.muted && !matches!(self.solo, TrackSoloState::Solo)
            || matches!(self.solo, TrackSoloState::Soloing)
    }
}

/// Track fields edited in place by the UI. Only `name` is stored in the
/// engine (and undoable); the rest is display state.
#[derive(Clone, Debug, PartialEq)]
pub struct MutableTrackCore {
    pub name: String,
    pub height: f32,
    /// Shown as a thin row; for a group, what's inside is hidden too.
    pub collapsed: bool,
    /// Height to go back to when opened.
    pub expanded_height: f32,
    pub color: Color32,
    pub arm: bool,
}

impl MutableTrackCore {
    pub fn new() -> Self {
        Self {
            collapsed: false,
            height: DEFAULT_TRACK_HEIGHT,
            expanded_height: DEFAULT_TRACK_HEIGHT,
            // New tracks get a palette colour from `ToniqueProjectState`.
            color: Color32::GRAY,
            name: "# Audio Track".into(),
            arm: false,
        }
    }
}
