use crate::audio::clip::ClipBackend;

pub struct TrackBackend {
    pub id: String,
    pub volume: f32,
    // Sorted by start position for quick access
    pub clips: Vec<ClipBackend>,
    pub muted: bool,
    previous_sample_index: Option<usize>,
}

impl TrackBackend {
    pub fn new(id: String, volume: f32) -> Self {
        TrackBackend {
            id,
            volume,
            clips: Vec::new(),
            previous_sample_index: None,
            muted: false,
        }
    }

    pub fn remove_clip(&mut self, id: String) -> Option<ClipBackend> {
        if let Some(i) = self.clips.iter().position(|clip| clip.id == id) {
            return Some(self.clips.remove(i));
        }
        None
    }

    pub fn seek(&mut self, position: usize) {
        for clip in self.clips.iter_mut() {
            if clip.start_frame <= position
                && position <= clip.start_frame + clip.stream.info().num_frames
            {
            } else if clip.start_frame > position {
            }
        }
    }
}
