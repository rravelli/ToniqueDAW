use crate::analysis::AudioInfo;
use std::{fmt::Debug, time::Duration};

/// A clip representing an audio file placed on a track
#[derive(Clone)]
pub struct ClipCore {
    pub id: String,
    /// Audio metadata
    pub audio: AudioInfo,
    /// Position in beat
    pub position: f32,
    /// Ratio of the trimmed start length over the original length
    /// Between 0 and 1
    pub trim_start: f32,
    /// Ratio of the trimmed end length over the original length
    /// Between 0 and 1
    pub trim_end: f32,
}

impl ClipCore {
    pub fn new(audio: AudioInfo, position: f32) -> Self {
        Self {
            id: uuid::Uuid::new_v4().into(),
            audio,
            position,
            trim_start: 0.,
            trim_end: 1.,
        }
    }

    pub fn clone_with_new_id(&self) -> Self {
        let mut clone = self.clone();
        clone.id = uuid::Uuid::new_v4().into();
        clone
    }

    pub fn duration(&self) -> Option<Duration> {
        if let Some(duration) = self.audio.duration {
            Some(Duration::from_secs_f32(
                duration.as_secs_f32() * (self.trim_end - self.trim_start),
            ))
        } else {
            None
        }
    }
    pub fn trim_start_at(&mut self, beats: f32, bpm: f32) {
        let duration = self.audio.duration.unwrap().as_secs_f32() * bpm / 60.;
        let clamped_beats = beats.clamp(self.position - duration * self.trim_start, self.end(bpm));
        self.trim_start += (clamped_beats - self.position) / duration;
        self.position = clamped_beats;

        self.trim_start = self.trim_start.clamp(0., 1.);
    }

    pub fn trim_end_at(&mut self, beats: f32, bpm: f32) {
        let duration = self.audio.duration.unwrap().as_secs_f32() * bpm / 60.;
        self.trim_end = (beats - self.position) / duration + self.trim_start;
        self.trim_end = self.trim_end.clamp(0., 1.);
    }

    pub fn end(&self, bpm: f32) -> f32 {
        self.position
            + self.audio.duration.unwrap().as_secs_f32() / 60.
                * bpm
                * (self.trim_end - self.trim_start)
    }
}

impl Debug for ClipCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipCore")
            .field("id", &self.id)
            .field("audio", &self.audio.name)
            .field("position", &self.position)
            .field("trim_start", &self.trim_start)
            .field("trim_end", &self.trim_end)
            .finish()
    }
}
