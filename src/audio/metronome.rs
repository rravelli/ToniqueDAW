use fundsp::hacker::{AudioUnit, envelope, square_hz};

pub struct MetronomeBackend {
    pub enabled: bool,
    pub metronome_volume: f32,
    pub metronome_tick: Box<dyn AudioUnit>,
    pub metronome_accent: Box<dyn AudioUnit>,
}

impl MetronomeBackend {
    pub fn new() -> Self {
        Self {
            enabled: false,
            metronome_volume: 0.4,
            metronome_tick: Box::new(metronome_click(1000.0)), // regular tick
            metronome_accent: Box::new(metronome_click(1800.0)), // bar accent}
        }
    }

    pub fn render(
        &mut self,
        output: &mut [f32],
        num_frames: usize,
        sample_rate: usize,
        playhead: usize,
        bpm: f32,
    ) {
        let samples_per_beat = (sample_rate as f32 * 60.0 / bpm) as usize;
        let beats_per_bar = 4;

        for i in 0..num_frames {
            let absolute_sample = playhead + i;

            // Determine current beat
            let beat_index = absolute_sample / samples_per_beat;
            let sample_in_beat = absolute_sample % samples_per_beat;

            // Only trigger at the start of each beat
            if sample_in_beat == 0 {
                let is_accent = beat_index % beats_per_bar == 0;
                let click = if is_accent {
                    &mut self.metronome_accent
                } else {
                    &mut self.metronome_tick
                };
                click.reset();
            }

            // Mix click output into the buffer
            let tick = self.metronome_tick.get_mono() * self.metronome_volume;
            let accent = self.metronome_accent.get_mono() * self.metronome_volume;
            let value = tick + accent;

            output[2 * i] += value;
            output[2 * i + 1] += value;
        }
    }
}
/// Click used
pub fn metronome_click(freq: f32) -> impl AudioUnit {
    let env = envelope(|t| {
        if t < 0.005 {
            t / 0.005 // fast attack
        } else {
            (-(t - 0.005) * 80.0).exp() // fast decay
        }
    });
    square_hz(freq) * env
}
