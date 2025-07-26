use std::path::PathBuf;

use crate::metrics::GlobalMetrics;

pub enum GuiToPlayerMsg {
    // Playback control messages
    Play,
    Pause,
    SeekTo(f32),
    // Track messages
    AddTrack(String),
    RemoveTrack(String),
    MuteTrack(String, bool),
    SoloTracks(Vec<String>),
    ChangeTrackVolume(String, f32),

    // Clip messages
    AddClip(String, PathBuf, f32, String, f32, f32), // (track_id, file_path, start_position, clip_id, trim_start, trim_end)
    RemoveClip(String, String),                      // clip id, track id
    MoveClip(String, String, f32),                   // clip id, track id, position
    ResizeClip(String, f32, f32),                    // clip_id, trim_start, trim_end
}

pub enum ProcessToGuiMsg {
    PlaybackPos(f32),
    Metrics(GlobalMetrics),
}
