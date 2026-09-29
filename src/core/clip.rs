use crate::analysis::AudioInfo;
use std::{fmt::Debug, time::Duration};
use tonique_engine::{edit::ClipId, time::BeatPos};

/// A clip representing an audio file placed on a track
#[derive(Clone)]
pub struct AudioClip {
    pub id: ClipId,
    /// Audio metadata
    pub audio: AudioInfo,
    pub position: BeatPos,
    /// Ratio of the trimmed start length over the original length
    /// Between 0 and 1
    pub trim_start: f32,
    /// Ratio of the trimmed end length over the original length
    /// Between 0 and 1
    pub trim_end: f32,
}

impl AudioClip {
    /// Get `id` from `ProjectState::new_clip_id`.
    pub fn new(id: ClipId, audio: AudioInfo, position: BeatPos) -> Self {
        Self {
            id,
            audio,
            position,
            trim_start: 0.,
            trim_end: 1.,
        }
    }

    pub fn with_id(&self, id: ClipId) -> Self {
        let mut clone = self.clone();
        clone.id = id;
        clone
    }

    /// Length of the whole source file, in seconds.
    pub fn source_seconds(&self) -> f32 {
        self.audio.duration.map_or(0., |d| d.as_secs_f32())
    }

    pub fn duration(&self) -> Option<Duration> {
        self.audio.duration.map(|duration| {
            Duration::from_secs_f32(duration.as_secs_f32() * (self.trim_end - self.trim_start))
        })
    }
    /// Move the clip's start to `beats`, keeping its end: no earlier than
    /// the start of the file or the first beat, no later than the end.
    pub fn trim_start_at(&mut self, beats: BeatPos, bpm: f32) {
        let duration = self.source_beats(bpm);
        let file_start = self.position - duration * self.trim_start as f64;
        let clamped_beats = beats.clamp(file_start.max(BeatPos::ZERO), self.end(bpm));
        self.trim_start += ((clamped_beats - self.position) / duration) as f32;
        self.position = clamped_beats;

        self.trim_start = self.trim_start.clamp(0., 1.);
    }

    pub fn trim_end_at(&mut self, beats: BeatPos, bpm: f32) {
        let duration = self.source_beats(bpm);
        self.trim_end = ((beats - self.position) / duration) as f32 + self.trim_start;
        self.trim_end = self.trim_end.clamp(0., 1.);
    }

    /// Keep only the part of the clip inside `start..end` (in beats). Never
    /// reveals audio that was trimmed away.
    pub fn crop(&mut self, start: BeatPos, end: BeatPos, bpm: f32) {
        let clip_end = self.end(bpm);
        self.trim_start_at(start.max(self.position), bpm);
        self.trim_end_at(end.min(clip_end), bpm);
    }

    pub fn end(&self, bpm: f32) -> BeatPos {
        self.position + self.source_beats(bpm) * (self.trim_end - self.trim_start) as f64
    }

    /// Length of the whole source file, in beats.
    fn source_beats(&self, bpm: f32) -> f64 {
        self.audio.duration.unwrap().as_secs_f64() * bpm as f64 / 60.
    }
}

impl Debug for AudioClip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioClip")
            .field("id", &self.id)
            .field("audio", &self.audio.name)
            .field("position", &self.position)
            .field("trim_start", &self.trim_start)
            .field("trim_end", &self.trim_end)
            .finish()
    }
}
