#[derive(Debug, Clone)]
pub struct MidiEvent {
    pub timestamp: usize,            // time since start of clip
    pub message: midly::MidiMessage, // or custom enum
}

#[derive(Debug, Clone)]
pub struct MidiClip {
    pub id: uuid::Uuid,
    pub name: String,
    pub start: usize,
    pub length: usize,          // clip length in musical time or real time
    pub events: Vec<MidiEvent>, // sorted by timestamp
}

impl MidiClip {
    pub fn in_range(&self, pos: usize, num_frames: usize) -> bool {
        self.start > pos + num_frames || self.start + self.length < pos
    }

    pub fn render_block(&self, pos: usize, num_frames: usize) {}
}
