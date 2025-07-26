use dashmap::DashMap;

#[derive(Clone)]
pub struct AudioMetrics {
    peak: [f32; 2],
    rms: [f32; 2],
    num_sample: usize,
}

impl AudioMetrics {
    pub fn new() -> Self {
        Self {
            peak: [0., 0.],
            rms: [0., 0.],
            num_sample: 0,
        }
    }

    pub fn add_sample(&mut self, value: f32, channel: usize) {
        self.peak[channel] = self.peak[channel].max(value);
        self.rms[channel] += value * value;
        self.num_sample += 1;
    }

    pub fn get_rms(&self) -> [f32; 2] {
        [
            (self.rms[0] / self.num_sample as f32).sqrt(),
            (self.rms[1] / self.num_sample as f32).sqrt(),
        ]
    }

    pub fn get_peak(&self) -> [f32; 2] {
        self.peak
    }
}

#[derive(Clone)]
pub struct GlobalMetrics {
    pub master: AudioMetrics,
    pub tracks: DashMap<String, AudioMetrics>,
    pub latency: f32,
}

impl GlobalMetrics {
    pub fn new() -> Self {
        Self {
            master: AudioMetrics::new(),
            tracks: DashMap::new(),
            latency: 0.,
        }
    }
}
