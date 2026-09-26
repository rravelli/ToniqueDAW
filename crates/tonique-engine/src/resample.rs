//! Resampling, split by cost:
//!
//! - File-rate mismatch: [`resample_offline`], high-quality windowed sinc,
//!   run once at load time off the RT thread.
//! - Device clock drift: [`HermiteResampler`] / [`LinearResampler`], cheap
//!   and RT-safe, steered by a [`DriftCorrector`].
//! - Musical varispeed: [`BackgroundVarispeedNode`], which does the DSP on a
//!   helper thread and hands audio to the RT thread through an SPSC ring.

use std::f64::consts::PI;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::graph::{Node, NodeProperties, ProcessContext};
use crate::param::AtomicParam;
use crate::sample::SampleBuffer;

const SINC_ZERO_CROSSINGS: f64 = 24.0;

/// Band-limited sample-rate conversion (Blackman-windowed sinc). Allocates;
/// never call on the RT thread.
pub fn resample_offline(input: &[f32], from_rate: f64, to_rate: f64) -> Vec<f32> {
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = to_rate / from_rate;
    let out_len = (input.len() as f64 * ratio).round() as usize;
    // Cutoff relative to the input Nyquist; below 1 when downsampling.
    let cutoff = ratio.min(1.0) * 0.95;
    let half_width = SINC_ZERO_CROSSINGS / cutoff;
    let reach = half_width.ceil() as isize;
    let mut out = Vec::with_capacity(out_len);
    for n in 0..out_len {
        let t = n as f64 / ratio;
        let centre = t.floor() as isize;
        let (mut acc, mut wsum) = (0.0f64, 0.0f64);
        for k in (centre - reach + 1)..=(centre + reach) {
            let x = t - k as f64;
            if x.abs() >= half_width {
                continue;
            }
            let arg = PI * x * cutoff;
            let sinc = if arg.abs() < 1e-12 { 1.0 } else { arg.sin() / arg };
            let u = x / half_width;
            let window = 0.42 + 0.5 * (PI * u).cos() + 0.08 * (2.0 * PI * u).cos();
            let w = sinc * window;
            wsum += w;
            if k >= 0 && (k as usize) < input.len() {
                acc += input[k as usize] as f64 * w;
            }
        }
        out.push(if wsum > 0.0 { (acc / wsum) as f32 } else { 0.0 });
    }
    out
}

/// Streaming, RT-safe resampler. `ratio` is input samples consumed per
/// output sample (1.0 = unity, 2.0 = twice as fast / an octave up).
pub trait Resampler: Send {
    /// Changed gradually inside `process` to avoid discontinuities.
    fn set_ratio(&mut self, ratio: f64);
    /// Returns `(consumed, produced)`. Stops when either side runs out.
    fn process(&mut self, input: &[f32], output: &mut [f32]) -> (usize, usize);
}

/// Per-output-sample one-pole glide towards the requested ratio.
#[derive(Clone, Copy, Debug)]
struct RatioGlide {
    current: f64,
    target: f64,
}

impl RatioGlide {
    const COEF: f64 = 0.001;
    fn new(r: f64) -> Self {
        Self { current: r, target: r }
    }
    #[inline]
    fn next(&mut self) -> f64 {
        self.current += (self.target - self.current) * Self::COEF;
        self.current
    }
}

/// Linear interpolation: cheapest, fine for tiny drift corrections.
pub struct LinearResampler {
    hist: [f32; 2],
    frac: f64,
    ratio: RatioGlide,
}

impl LinearResampler {
    pub fn new(ratio: f64) -> Self {
        Self { hist: [0.0; 2], frac: 1.0, ratio: RatioGlide::new(ratio) }
    }
}

impl Resampler for LinearResampler {
    fn set_ratio(&mut self, ratio: f64) {
        self.ratio.target = ratio;
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) -> (usize, usize) {
        let (mut consumed, mut produced) = (0, 0);
        while produced < output.len() {
            while self.frac >= 1.0 {
                let Some(&x) = input.get(consumed) else { return (consumed, produced) };
                self.hist = [self.hist[1], x];
                consumed += 1;
                self.frac -= 1.0;
            }
            let f = self.frac as f32;
            output[produced] = self.hist[0] + (self.hist[1] - self.hist[0]) * f;
            produced += 1;
            self.frac += self.ratio.next();
        }
        (consumed, produced)
    }
}

/// 4-point, 3rd-order Hermite interpolation: a good cheap default.
pub struct HermiteResampler {
    hist: [f32; 4],
    frac: f64,
    ratio: RatioGlide,
}

impl HermiteResampler {
    pub fn new(ratio: f64) -> Self {
        Self { hist: [0.0; 4], frac: 1.0, ratio: RatioGlide::new(ratio) }
    }
}

#[inline]
fn hermite(x: [f32; 4], t: f32) -> f32 {
    let c0 = x[1];
    let c1 = 0.5 * (x[2] - x[0]);
    let c2 = x[0] - 2.5 * x[1] + 2.0 * x[2] - 0.5 * x[3];
    let c3 = 0.5 * (x[3] - x[0]) + 1.5 * (x[1] - x[2]);
    ((c3 * t + c2) * t + c1) * t + c0
}

impl Resampler for HermiteResampler {
    fn set_ratio(&mut self, ratio: f64) {
        self.ratio.target = ratio;
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) -> (usize, usize) {
        let (mut consumed, mut produced) = (0, 0);
        while produced < output.len() {
            while self.frac >= 1.0 {
                let Some(&x) = input.get(consumed) else { return (consumed, produced) };
                self.hist = [self.hist[1], self.hist[2], self.hist[3], x];
                consumed += 1;
                self.frac -= 1.0;
            }
            output[produced] = hermite(self.hist, self.frac as f32);
            produced += 1;
            self.frac += self.ratio.next();
        }
        (consumed, produced)
    }
}

/// PI controller that nudges a resampling ratio so that a FIFO between two
/// clock domains stays near its target fill level.
#[derive(Clone, Debug)]
pub struct DriftCorrector {
    nominal: f64,
    target_fill: f64,
    integral: f64,
    kp: f64,
    ki: f64,
    max_deviation: f64,
}

impl DriftCorrector {
    pub fn new(nominal_ratio: f64, target_fill: usize) -> Self {
        Self { nominal: nominal_ratio, target_fill: target_fill as f64, integral: 0.0, kp: 1e-6, ki: 1e-9, max_deviation: 0.005 }
    }

    /// Call once per block with the FIFO's current fill; returns the ratio
    /// to feed to the resampler that drains it. Too full -> consume faster.
    pub fn update(&mut self, fill: usize) -> f64 {
        let err = fill as f64 - self.target_fill;
        self.integral = (self.integral + err).clamp(-1e6, 1e6);
        let adj = (self.kp * err + self.ki * self.integral).clamp(-self.max_deviation, self.max_deviation);
        self.nominal * (1.0 + adj)
    }
}

/// Plays a sample at a variable speed. The resampling runs on a helper
/// thread, a fixed lookahead ahead of playback; `process` only pops from an
/// SPSC ring. Trades `lookahead` frames of latency for RT headroom — the
/// same shape a real-time time-stretcher would have.
///
/// Free-running (loops the sample and ignores the transport): it exists to
/// demonstrate the threading pattern.
pub struct BackgroundVarispeedNode {
    rx: rtrb::Consumer<[f32; 2]>,
    stop: Arc<AtomicBool>,
    underruns: Arc<AtomicUsize>,
    worker: Option<JoinHandle<()>>,
}

impl BackgroundVarispeedNode {
    pub fn new(source: Arc<SampleBuffer>, speed: Arc<AtomicParam>, lookahead: usize) -> Self {
        let (mut tx, rx) = rtrb::RingBuffer::<[f32; 2]>::new(lookahead.max(64));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = std::thread::Builder::new()
            .name("tonique-varispeed".into())
            .spawn(move || {
                let chans = source.num_channels();
                let mut rs: Vec<HermiteResampler> = (0..2).map(|_| HermiteResampler::new(speed.get() as f64)).collect();
                let mut read_pos = 0usize;
                let (mut inbuf, mut outbuf) = (vec![0.0f32; 128 * 8 + 8], [vec![0.0f32; 128], vec![0.0f32; 128]]);
                while !worker_stop.load(Ordering::Relaxed) {
                    if tx.slots() < 128 {
                        std::thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    let speed_now = speed.get().clamp(0.05, 8.0) as f64;
                    let mut consumed = 0;
                    for (ch, r) in rs.iter_mut().enumerate() {
                        r.set_ratio(speed_now);
                        // Enough input for 128 outputs at the max speed.
                        let src = source.channel(ch.min(chans - 1));
                        for (i, s) in inbuf.iter_mut().enumerate() {
                            *s = src[(read_pos + i) % src.len()];
                        }
                        let (c, _p) = r.process(&inbuf, &mut outbuf[ch]);
                        consumed = c;
                    }
                    read_pos = (read_pos + consumed) % source.len();
                    for (&l, &r) in outbuf[0].iter().zip(&outbuf[1]) {
                        let _ = tx.push([l, r]);
                    }
                }
            })
            .expect("spawn varispeed worker");
        Self { rx, stop, underruns: Arc::new(AtomicUsize::new(0)), worker: Some(worker) }
    }

    pub fn underrun_counter(&self) -> Arc<AtomicUsize> {
        self.underruns.clone()
    }
}

impl Node for BackgroundVarispeedNode {
    fn properties(&self) -> NodeProperties {
        NodeProperties::audio(2)
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        let (l, r) = ctx.audio_out.channel_pair_mut(0, 1);
        for (l, r) in l.iter_mut().zip(r.iter_mut()) {
            match self.rx.pop() {
                Ok([a, b]) => {
                    *l = a;
                    *r = b;
                }
                Err(_) => {
                    self.underruns.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

impl Drop for BackgroundVarispeedNode {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: f64, len: usize) -> Vec<f32> {
        (0..len).map(|i| (2.0 * PI * freq * i as f64 / rate).sin() as f32).collect()
    }

    #[test]
    fn offline_preserves_a_sine() {
        let input = sine(1000.0, 48000.0, 4800);
        let out = resample_offline(&input, 48000.0, 44100.0);
        assert_eq!(out.len(), 4410);
        let expected = sine(1000.0, 44100.0, 4410);
        // Ignore edges (filter ramp-in/out).
        let err = out[200..4200].iter().zip(&expected[200..4200]).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 2e-3, "max err {err}");
    }

    #[test]
    fn offline_downsampling_removes_content_above_nyquist() {
        // 20 kHz at 48k -> 22.05k: above the new Nyquist, should be filtered out.
        let input = sine(20000.0, 48000.0, 9600);
        let out = resample_offline(&input, 48000.0, 22050.0);
        let peak = out[500..out.len() - 500].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak < 0.01, "alias peak {peak}");
    }

    #[test]
    fn streaming_resamplers_track_a_sine() {
        for which in 0..2 {
            let input = sine(440.0, 48000.0, 48000);
            let mut out = vec![0.0; 44100];
            let mut rs: Box<dyn Resampler> =
                if which == 0 { Box::new(LinearResampler::new(48000.0 / 44100.0)) } else { Box::new(HermiteResampler::new(48000.0 / 44100.0)) };
            // Feed in uneven chunks to exercise state carried across calls.
            let (mut ci, mut co) = (0, 0);
            while co < out.len() && ci < input.len() {
                let chunk = (ci + 777).min(input.len());
                let (c, p) = rs.process(&input[ci..chunk], &mut out[co..(co + 500).min(44100)]);
                ci += c;
                co += p;
            }
            let latency = if which == 0 { 1.0 } else { 2.0 };
            let err = (1000..40000)
                .map(|n| {
                    let t = (n as f64 * 48000.0 / 44100.0 - latency) / 48000.0;
                    (out[n] - (2.0 * PI * 440.0 * t).sin() as f32).abs()
                })
                .fold(0.0, f32::max);
            assert!(err < if which == 0 { 5e-3 } else { 1e-3 }, "resampler {which} err {err}");
        }
    }

    #[test]
    fn drift_corrector_pulls_fill_towards_target() {
        // Producer at 48000 Hz, consumer resamples at `ratio` per output
        // sample while emitting 48010 samples/s (a slightly faster clock).
        let mut dc = DriftCorrector::new(48000.0 / 48010.0, 4800);
        let mut fill = 9600.0f64;
        let mut ratio = 48000.0 / 48010.0;
        for _ in 0..20000 {
            fill += 480.0 - 480.1 * ratio; // 10 ms blocks
            ratio = dc.update(fill.max(0.0) as usize);
        }
        assert!((fill - 4800.0).abs() < 300.0, "fill {fill}");
    }
}
