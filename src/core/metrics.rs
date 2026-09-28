use rustfft::{FftPlanner, num_complex::Complex};
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
    /// Most recent samples, for the spectrum view.
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

    /// Pull the latest levels and samples from an engine channel meter.
    /// Keeps the previous levels if no audio was processed since the last call.
    pub fn update(&mut self, meter: &ChannelMeter) {
        if let Some(levels) = meter.take_levels() {
            for ch in 0..2 {
                self.peak[ch] = levels[ch].peak;
                self.rms[ch] = self.alpha * levels[ch].rms + (1. - self.alpha) * self.rms[ch];
            }
        }
        for ch in 0..2 {
            self.samples[ch].resize(SCOPE_LEN, 0.);
            meter.read_scope(ch, &mut self.samples[ch]);
        }
    }

    pub fn spectrum(&mut self) -> Vec<f32> {
        let n = self.samples[0].len();
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(n);

        let hann: Vec<f32> = (0..n)
            .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (n as f32 - 1.0)).cos()))
            .collect();

        let mut buffer: Vec<Complex<f32>> = self.samples[0]
            .iter()
            .zip(hann.iter())
            .map(|(&x, &w)| Complex::new(x * w, 0.0))
            .collect();

        fft.process(&mut buffer);

        let window_sum = hann.iter().sum::<f32>();

        let spectrum: Vec<f32> = buffer
            .iter()
            .take(n / 2)
            .map(|c| 20.0 * (c.norm() * 2.0 / window_sum).max(1e-9).log10() / 4. + 10.) // dBFS
            .collect();

        spectrum
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
