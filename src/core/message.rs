use crate::{core::clip::ClipCore, metrics::GlobalMetrics};
use fundsp::hacker::AudioUnit;
use std::{collections::HashMap, fmt::Debug, path::PathBuf};

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
    AddClips(HashMap<String, Vec<ClipCore>>),
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

impl Debug for GuiToPlayerMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Play => write!(f, "Play"),
            Self::Pause => write!(f, "Pause"),
            Self::SeekTo(arg0) => f.debug_tuple("SeekTo").field(arg0).finish(),
            Self::PlayPreview(arg0) => f.debug_tuple("PlayPreview").field(arg0).finish(),
            Self::PausePreview() => f.debug_tuple("PausePreview").finish(),
            Self::SeekPreview(arg0) => f.debug_tuple("SeekPreview").field(arg0).finish(),
            Self::UpdateBPM(arg0) => f.debug_tuple("UpdateBPM").field(arg0).finish(),
            Self::AddTrack(arg0) => f.debug_tuple("AddTrack").field(arg0).finish(),
            Self::RemoveTrack(arg0) => f.debug_tuple("RemoveTrack").field(arg0).finish(),
            Self::MuteTrack(arg0, arg1) => {
                f.debug_tuple("MuteTrack").field(arg0).field(arg1).finish()
            }
            Self::SoloTracks(arg0) => f.debug_tuple("SoloTracks").field(arg0).finish(),
            Self::ChangeTrackVolume(arg0, arg1) => f
                .debug_tuple("ChangeTrackVolume")
                .field(arg0)
                .field(arg1)
                .finish(),
            Self::AddNode(arg0, arg1, arg2, arg3) => f
                .debug_tuple("AddNode")
                .field(arg0)
                .field(arg1)
                .field(arg2)
                .finish(),
            Self::RemoveNode(arg0, arg1) => {
                f.debug_tuple("RemoveNode").field(arg0).field(arg1).finish()
            }
            Self::SetNodeEnabled(arg0, arg1, arg2) => f
                .debug_tuple("SetNodeEnabled")
                .field(arg0)
                .field(arg1)
                .field(arg2)
                .finish(),
            Self::AddClip(arg0, arg1, arg2, arg3, arg4, arg5) => f
                .debug_tuple("AddClip")
                .field(arg0)
                .field(arg1)
                .field(arg2)
                .field(arg3)
                .field(arg4)
                .field(arg5)
                .finish(),
            Self::AddClips(arg0) => f.debug_tuple("AddClips").field(arg0).finish(),
            Self::RemoveClip(arg0) => f.debug_tuple("RemoveClip").field(arg0).finish(),
            Self::MoveClip(arg0, arg1, arg2) => f
                .debug_tuple("MoveClip")
                .field(arg0)
                .field(arg1)
                .field(arg2)
                .finish(),
            Self::ResizeClip(arg0, arg1, arg2, arg3) => f
                .debug_tuple("ResizeClip")
                .field(arg0)
                .field(arg1)
                .field(arg2)
                .field(arg3)
                .finish(),
            Self::ResizeClips { track_id, clips } => f
                .debug_struct("ResizeClips")
                .field("track_id", track_id)
                .field("clips", clips)
                .finish(),
            Self::DuplicateTrack {
                id,
                new_id,
                clip_map,
            } => f
                .debug_struct("DuplicateTrack")
                .field("id", id)
                .field("new_id", new_id)
                .field("clip_map", clip_map)
                .finish(),
        }
    }
}
