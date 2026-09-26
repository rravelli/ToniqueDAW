use std::time::Duration;

use tonique_engine::engine::{AudioProcessor, Engine, EngineConfig, render_offline};
use tonique_engine::preview::PreviewSource;

const SR: f64 = 48000.0;
const STEP: f32 = 1e-4;

/// Frame `i` has the value `i * STEP` on the left and its negation on the right.
struct Ramp {
    len: usize,
    pos: usize,
    rate: f64,
}

impl PreviewSource for Ramp {
    fn sample_rate(&self) -> f64 {
        self.rate
    }
    fn seek(&mut self, frame: usize) {
        self.pos = frame.min(self.len);
    }
    fn read(&mut self, left: &mut [f32], right: &mut [f32]) -> usize {
        let n = left.len().min(self.len - self.pos);
        for i in 0..n {
            left[i] = (self.pos + i) as f32 * STEP;
            right[i] = -left[i];
        }
        self.pos += n;
        n
    }
}

fn ramp(len: usize, rate: f64) -> Box<Ramp> {
    Box::new(Ramp { len, pos: 0, rate })
}

fn engine() -> (Engine, AudioProcessor) {
    Engine::new(EngineConfig { sample_rate: SR, housekeeping_thread: false, ..Default::default() })
}

/// Let the feeder thread fill the ring, and pick up the command.
fn settle(p: &mut AudioProcessor) -> Vec<f32> {
    std::thread::sleep(Duration::from_millis(100));
    render_offline(p, 0, 2)
}

fn left(out: &[f32]) -> Vec<f32> {
    out.iter().step_by(2).copied().collect()
}

#[test]
fn plays_the_source_while_the_transport_is_stopped() {
    let (mut e, mut p) = engine();
    e.preview_play(ramp(10_000, SR), 0).unwrap();
    assert!(e.is_previewing());
    settle(&mut p);
    let out = render_offline(&mut p, 4096, 2);
    for (i, frame) in out.chunks(2).enumerate() {
        assert!((frame[0] - i as f32 * STEP).abs() < 1e-6, "frame {i}: {}", frame[0]);
        assert_eq!(frame[1], -frame[0]);
    }
    assert_eq!(e.preview_position(), Some(4096));
    assert!(!e.is_playing(), "the transport stays stopped");
}

#[test]
fn seek_drops_queued_audio_and_stop_silences() {
    let (mut e, mut p) = engine();
    e.preview_play(ramp(100_000, SR), 0).unwrap();
    settle(&mut p);
    render_offline(&mut p, 256, 2);

    e.preview_seek(50_000).unwrap();
    assert_eq!(e.preview_position(), Some(50_000));
    settle(&mut p);
    let out = left(&render_offline(&mut p, 256, 2));
    assert!((out[0] - 50_000.0 * STEP).abs() < 1e-5, "resumed at {}", out[0] / STEP);

    e.preview_stop().unwrap();
    assert!(!e.is_previewing());
    assert_eq!(e.preview_position(), None);
    settle(&mut p);
    assert!(render_offline(&mut p, 256, 2).iter().all(|s| *s == 0.0));
    assert!(e.collect_garbage() > 0, "old streams are freed off the audio thread");
}

#[test]
fn finishes_at_the_end_of_the_source() {
    let (mut e, mut p) = engine();
    e.preview_play(ramp(1000, SR), 0).unwrap();
    settle(&mut p);
    let out = left(&render_offline(&mut p, 2048, 2));
    assert!(out[999] > 0.0);
    assert!(out[1000..].iter().all(|s| *s == 0.0));
    assert!(!e.is_previewing());
    assert_eq!(e.preview_position(), None);
}

#[test]
fn resamples_to_the_engine_rate() {
    let (mut e, mut p) = engine();
    e.preview_play(ramp(1000, SR / 2.0), 0).unwrap(); // twice as many engine frames
    settle(&mut p);
    let out = left(&render_offline(&mut p, 1000, 2));
    // Halfway through the output, halfway through the source.
    assert!((out[500] / STEP - 250.0).abs() < 3.0, "at {}", out[500] / STEP);
    assert_eq!(e.preview_position(), Some(500));
    render_offline(&mut p, 1100, 2);
    assert!(!e.is_previewing());
}

#[test]
fn seeking_before_any_play_does_nothing() {
    let (mut e, _p) = engine();
    e.preview_seek(10).unwrap();
    assert!(!e.is_previewing());
}
