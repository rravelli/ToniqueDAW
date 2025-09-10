use dashmap::DashMap;
use rustfft::{FftPlanner, num_complex::Complex};

#[derive(Clone)]
pub struct AudioMetrics {
    peak: [f32; 2],
    rms: [f32; 2],
    prev_rms: [f32; 2],
    /// Smoothing factor
    alpha: f32,
    pub samples: [Vec<f32>; 2],
}

impl AudioMetrics {
    pub fn new() -> Self {
        Self {
            peak: [0., 0.],
            rms: [0., 0.],
            prev_rms: [0., 0.],
            alpha: 0.6,
            samples: [vec![], vec![]],
        }
    }

    pub fn reset(&mut self) {
        if !self.samples[0].is_empty() {
            self.prev_rms = self.get_rms();
        }
        self.peak = [0., 0.];
        self.rms = [0., 0.];
        self.samples[0].clear();
        self.samples[1].clear();
    }

    pub fn add_sample(&mut self, value: f32, channel: usize) {
        self.peak[channel] = self.peak[channel].max(value);
        self.rms[channel] += value * value;
        self.samples[channel].push(value);
    }

    pub fn get_fft(&mut self) -> Vec<f32> {
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

    fn compute_rms(&self) -> [f32; 2] {
        let num_sample = self.samples[0].len();
        [
            (self.rms[0] / num_sample as f32).sqrt(),
            (self.rms[1] / num_sample as f32).sqrt(),
        ]
    }

    fn smooth(&self, val: [f32; 2], prev: [f32; 2]) -> [f32; 2] {
        [
            self.alpha * val[0] + (1. - self.alpha) * prev[0],
            self.alpha * val[1] + (1. - self.alpha) * prev[1],
        ]
    }

    pub fn get_rms(&self) -> [f32; 2] {
        // smooth rms
        self.smooth(self.compute_rms(), self.prev_rms)
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
