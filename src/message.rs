use std::{collections::HashMap, path::PathBuf};

use fundsp::hacker::AudioUnit;

use crate::metrics::GlobalMetrics;

pub enum GuiToPlayerMsg {
    // Playback control messages
    Play,
    Pause,
    SeekTo(f32),
    PlayPreview(PathBuf),
    PausePreview(),
    SeekPreview(usize),

    UpdateBPM(f32),
    // Track messages
    AddTrack(String),
    RemoveTrack(String),
    MuteTrack(String, bool),
    SoloTracks(Vec<String>),
    ChangeTrackVolume(String, f32),

    // Effect messages
    AddNode(String, usize, String, Box<dyn AudioUnit>), // track_id, index, node, node_id
    RemoveNode(String, String),                         // track_id, node_id
    SetNodeEnabled(String, String, bool),               // track_id, node_id, enabled

    // Clip messages
    AddClip(String, PathBuf, f32, String, f32, f32), // (track_id, file_path, start_position, clip_id, trim_start, trim_end)
    AddClips(Vec<CreateClipCommand>),
    RemoveClip(Vec<String>),           // Vec<clip id>
    MoveClip(String, String, f32),     // clip id, track id, position
    ResizeClip(String, f32, f32, f32), // clip_id, trim_start, trim_end
    ResizeClips {
        track_id: String,
        clips: HashMap<String, (f32, f32)>,
    },
    DuplicateTrack {
        /// Track id to duplicate
        id: String,
        /// The duplicated track id
        new_id: String,
        /// Mapping between clips and duplicated clips ids
        clip_map: HashMap<String, String>,
    },
}

pub enum ProcessToGuiMsg {
    PlaybackPos(f32),
    PreviewPos(usize),
    Metrics(GlobalMetrics),
}

pub struct CreateClipCommand {
    pub track_id: String,
    pub clip_id: String,
    pub file_path: PathBuf,
    pub position: f32,
    pub trim_start: f32,
    pub trim_end: f32,
}
