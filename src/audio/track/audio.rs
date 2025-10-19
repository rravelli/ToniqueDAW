use crate::audio::{clip::ClipBackend, track::Processor};

#[derive(Clone)]
pub struct AudioTrackData {
    pub clips: Vec<ClipBackend>,
}

impl AudioTrackData {
    pub fn new() -> Self {
        Self { clips: Vec::new() }
    }
}

impl Processor for AudioTrackData {
    fn process(&mut self, pos: usize, num_frames: usize, sample_rate: usize, mix: &mut Vec<f32>) {
        for clip in self.clips.iter_mut() {
            let clip_start = clip.start_frame;
            let clip_end = clip.end(sample_rate);
            // not in range
            if pos > clip_end || clip_start > pos + num_frames {
                continue;
            }
            // not ready
            if let Ok(ready) = clip.audio.ready.read()
                && !*ready
            {
                continue;
            };

            clip.render_block(mix, pos, num_frames, sample_rate);
        }
    }
}
