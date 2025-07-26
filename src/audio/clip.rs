use std::path::PathBuf;

use creek::{ReadDiskStream, SymphoniaDecoder};

use crate::{analysis::AudioInfo, cache::AUDIO_ANALYSIS_CACHE};

pub struct ClipBackend {
    pub id: String,
    pub audio: AudioInfo,
    pub start_frame: usize,
    pub trim_start: f32,
    pub trim_end: f32,
    // To be deleted
    pub stream: Box<ReadDiskStream<SymphoniaDecoder>>,
}

impl ClipBackend {
    pub fn new(
        id: String,
        path: PathBuf,
        start_frame: usize,
        trim_start: f32,
        trim_end: f32,
    ) -> Self {
        let audio = AUDIO_ANALYSIS_CACHE.get_or_analyze(path.clone()).unwrap();

        let stream =
            Box::new(ReadDiskStream::<SymphoniaDecoder>::new(path, 0, Default::default()).unwrap());

        Self {
            id,
            audio,
            start_frame,
            stream,
            trim_start: trim_start,
            trim_end: trim_end,
        }
    }

    pub fn num_frames(&self) -> usize {
        self.playhead_end() - self.playhead_start() as usize
    }

    pub fn playhead_start(&self) -> usize {
        (self.trim_start * self.stream.info().num_frames as f32).round() as usize
    }

    pub fn playhead_end(&self) -> usize {
        (self.trim_end * self.stream.info().num_frames as f32).round() as usize
    }

    pub fn end(&self, sample_rate: usize) -> usize {
        self.start_frame
            + (self.num_frames() as f32 * sample_rate as f32 / self.audio.sample_rate as f32)
                .round() as usize
    }
}

impl Clone for ClipBackend {
    fn clone(&self) -> Self {
        let stream = Box::new(
            ReadDiskStream::<SymphoniaDecoder>::new(self.audio.path.clone(), 0, Default::default())
                .unwrap(),
        );

        Self {
            id: self.id.clone(),
            audio: self.audio.clone(),
            start_frame: self.start_frame.clone(),
            stream,
            trim_start: self.trim_start,
            trim_end: self.trim_end,
        }
    }
}
