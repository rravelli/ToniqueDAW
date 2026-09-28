use crate::core::clip::AudioClip;
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

/// Read-only snapshot of a row of the track list (a track or a group) for
/// the UI, built from the engine's edit.
#[derive(Debug, Clone)]
pub struct TrackRow {
    pub id: TrackId,
    pub clips: Vec<AudioClip>,
    pub muted: bool,
    pub volume: f32,
    /// Armed for recording.
    pub armed: bool,
    pub name: String,
    pub height: f32,
    pub collapsed: bool,
    pub color: Color32,
    pub selected: bool,
    pub solo: TrackSoloState,
    /// Position in the engine's track list of its first track: the track
    /// itself, or a group's first.
    pub first_track_index: usize,
    pub kind: TrackKind,
    /// Groups it's nested in.
    pub depth: usize,
}

impl TrackRow {
    pub fn is_silenced(&self) -> bool {
        self.muted && !matches!(self.solo, TrackSoloState::Solo)
            || matches!(self.solo, TrackSoloState::Soloing)
    }
}

/// Track fields edited in place by the UI. Only `name` is stored in the
/// engine (and undoable); the rest is display state.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackView {
    pub name: String,
    pub height: f32,
    /// Shown as a thin row; for a group, what's inside is hidden too.
    pub collapsed: bool,
    /// Height to go back to when opened.
    pub expanded_height: f32,
    pub color: Color32,
    /// Armed for recording.
    pub armed: bool,
}

impl TrackView {
    pub fn new() -> Self {
        Self {
            collapsed: false,
            height: DEFAULT_TRACK_HEIGHT,
            expanded_height: DEFAULT_TRACK_HEIGHT,
            // New tracks get a palette colour from `ProjectState`.
            color: Color32::GRAY,
            name: "# Audio Track".into(),
            armed: false,
        }
    }
}
