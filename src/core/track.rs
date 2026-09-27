use crate::core::clip::ClipCore;
use egui::Color32;
use tonique_engine::edit::TrackId;

#[derive(Debug, Clone)]
pub enum TrackSoloState {
    Soloing,
    NotSoloing,
    Solo,
}

pub const DEFAULT_TRACK_HEIGHT: f32 = 60.;
pub const TRACK_CLOSED_HEIGHT: f32 = 22.;

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
    pub closed: bool,
    pub color: Color32,
    pub selected: bool,
    pub solo: TrackSoloState,
    pub index: usize,
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
    pub closed: bool,
    pub color: Color32,
    pub arm: bool,
}

impl MutableTrackCore {
    pub fn new() -> Self {
        Self {
            closed: false,
            height: DEFAULT_TRACK_HEIGHT,
            // New tracks get a palette colour from `ToniqueProjectState`.
            color: Color32::GRAY,
            name: "# Audio Track".into(),
            arm: false,
        }
    }
}
