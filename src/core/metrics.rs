use std::collections::HashMap;
use tonique_engine::{
    edit::TrackId,
    meter::{ChannelMeter, SCOPE_LEN},
};

#[derive(Clone)]
pub struct AudioMetrics {
    peak: [f32; 2],
    rms: [f32; 2],
    /// Smoothing factor
    alpha: f32,
    /// Most recent samples, for the scope (the master's only).
    pub samples: [Vec<f32>; 2],
}

impl AudioMetrics {
    pub fn new() -> Self {
        Self {
            peak: [0., 0.],
            rms: [0., 0.],
            alpha: 0.6,
            samples: [vec![], vec![]],
        }
    }

    /// Pull the latest levels from an engine channel meter, and its recent
    /// samples if `scope`: only the master's are shown, and copying every
    /// channel's each frame adds up. Keeps the previous levels if no audio
    /// was processed since the last call.
    pub fn update(&mut self, meter: &ChannelMeter, scope: bool) {
        if let Some(levels) = meter.take_levels() {
            for (ch, level) in levels.iter().enumerate() {
                self.peak[ch] = level.peak;
                self.rms[ch] = self.alpha * level.rms + (1. - self.alpha) * self.rms[ch];
            }
        }
        if scope {
            for ch in 0..2 {
                self.samples[ch].resize(SCOPE_LEN, 0.);
                meter.read_scope(ch, &mut self.samples[ch]);
            }
        }
    }

    pub fn rms(&self) -> [f32; 2] {
        self.rms
    }

    pub fn peak(&self) -> [f32; 2] {
        self.peak
    }
}

#[derive(Clone)]
pub struct GlobalMetrics {
    pub master: AudioMetrics,
    pub tracks: HashMap<TrackId, AudioMetrics>,
    pub latency: f32,
}

impl GlobalMetrics {
    pub fn new() -> Self {
        Self {
            master: AudioMetrics::new(),
            tracks: HashMap::new(),
            latency: 0.,
        }
    }
}
