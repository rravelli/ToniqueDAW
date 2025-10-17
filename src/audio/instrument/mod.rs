use fundsp::hacker::AudioUnit;

struct Instrument {
    unit: Box<dyn AudioUnit>,
    voices: Vec<Box<dyn AudioUnit>>,
}

impl Instrument {
    pub fn new(unit: Box<dyn AudioUnit>, voices: usize) -> Self {
        Self {
            unit,
            voices: Vec::new(),
        }
    }

    pub fn reset(&mut self) {}

    pub fn render_block(&mut self, pos: usize, num_frames: usize, sample_rate: usize) {}
}
