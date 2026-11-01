//! Algorithmic reverb: a pre-delay, allpass diffusers, then a feedback
//! delay network of eight damped lines mixed by a Householder matrix.

use std::sync::Arc;

use crate::graph::{Node, NodeIdentity, NodeProperties, ProcessContext, StateTransfer};
use crate::param::{AtomicParam, Smoother};

const LINES: usize = 8;
/// Line lengths at the largest size, in ms: far enough from each other's
/// multiples not to ring together.
const LINE_MS: [f32; LINES] = [37.3, 41.9, 47.7, 53.1, 59.3, 66.1, 71.7, 79.3];
/// The smallest room is this much of the largest.
const MIN_SIZE: f32 = 0.2;
/// Diffusers on the way in, per channel, in ms; the right's a little
/// longer so the sides differ.
const DIFFUSER_MS: [f32; 4] = [4.77, 3.59, 12.73, 9.31];
const DIFFUSER_SPREAD: f32 = 1.07;
const DIFFUSION: f32 = 0.6;
/// Longest pre-delay, in ms: the top of its parameter.
pub const REVERB_MAX_PREDELAY_MS: f32 = 200.0;
/// Which lines go to the left and right, and with which sign: every line
/// is heard on both sides, differently.
const OUT_LEFT: [f32; LINES] = [1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
const OUT_RIGHT: [f32; LINES] = [1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0];
const WET_GAIN: f32 = 0.35;

/// A delay line read anywhere between its samples.
#[derive(Clone, Default)]
struct Line {
    buf: Vec<f32>,
    pos: usize,
}

impl Line {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(2)],
            pos: 0,
        }
    }

    /// The sample written `delay` samples ago, at least 1 and less than
    /// the line's length.
    #[inline]
    fn read(&self, delay: f32) -> f32 {
        let len = self.buf.len();
        let delay = delay.clamp(1.0, (len - 1) as f32);
        let whole = delay as usize;
        let frac = delay - whole as f32;
        let a = self.buf[(self.pos + len - whole) % len];
        let b = self.buf[(self.pos + len - whole - 1) % len];
        a + (b - a) * frac
    }

    #[inline]
    fn write(&mut self, x: f32) {
        self.pos = (self.pos + 1) % self.buf.len();
        self.buf[self.pos] = x;
    }
}

/// Schroeder allpass: smears a sound in time without colouring it.
#[derive(Clone, Default)]
struct Allpass {
    buf: Vec<f32>,
    pos: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            pos: 0,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let delayed = self.buf[self.pos];
        let v = x + DIFFUSION * delayed;
        self.buf[self.pos] = v;
        self.pos = (self.pos + 1) % self.buf.len();
        delayed - DIFFUSION * v
    }
}

/// Stereo reverb: size (0 to 1), decay (RT60, s), damping (0 to 1),
/// pre-delay (ms) and dry/wet mix (0 to 1).
pub struct ReverbNode {
    params: [Arc<AtomicParam>; 5],
    /// Size, decay, damping, pre-delay, mix.
    smoothers: [Smoother; 5],
    predelay: [Line; 2],
    diffusers: [[Allpass; 4]; 2],
    lines: [Line; LINES],
    /// Low-pass memory of each line.
    damped: [f32; LINES],
    sample_rate: f32,
    identity: Option<NodeIdentity>,
}

impl ReverbNode {
    pub fn new([size, decay, damping, predelay, mix]: [Arc<AtomicParam>; 5]) -> Self {
        let params = [size, decay, damping, predelay, mix];
        Self {
            smoothers: std::array::from_fn(|i| Smoother::new(params[i].get())),
            params,
            predelay: Default::default(),
            diffusers: Default::default(),
            lines: Default::default(),
            damped: [0.0; LINES],
            sample_rate: 0.0,
            identity: None,
        }
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }

    fn samples(&self, ms: f32) -> usize {
        (ms * 0.001 * self.sample_rate).ceil() as usize
    }
}

impl Node for ReverbNode {
    fn properties(&self) -> NodeProperties {
        let p = NodeProperties::audio(2);
        match self.identity {
            Some(id) => p.with_identity(id),
            None => p,
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block: usize) {
        self.sample_rate = sample_rate as f32;
        let predelay = self.samples(REVERB_MAX_PREDELAY_MS) + 2;
        self.predelay = [Line::new(predelay), Line::new(predelay)];
        self.diffusers = std::array::from_fn(|ch| {
            let spread = if ch == 0 { 1.0 } else { DIFFUSER_SPREAD };
            DIFFUSER_MS.map(|ms| Allpass::new(self.samples(ms * spread)))
        });
        self.lines = LINE_MS.map(|ms| Line::new(self.samples(ms) + 2));
        self.damped = [0.0; LINES];
        for (s, p) in self.smoothers.iter_mut().zip(&self.params) {
            s.set_sample_rate(sample_rate);
            s.snap(p);
        }
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        ctx.sum_inputs_to_output();
        for (s, p) in self.smoothers.iter_mut().zip(&self.params) {
            s.retarget(p);
        }
        let sr = self.sample_rate;
        // Per block: what each line feeds back for the decay to take
        // RT60 to fall by 60 dB, at the size it starts the block at.
        let [size, decay, damping, ..] = self.smoothers.each_ref().map(|s| s.current());
        let scale = MIN_SIZE + (1.0 - MIN_SIZE) * size.clamp(0.0, 1.0);
        let feedback = LINE_MS.map(|ms| {
            let seconds = ms * 0.001 * scale;
            10f32.powf(-3.0 * seconds / decay.max(0.05))
        });
        let damping = damping.clamp(0.0, 1.0) * 0.85;

        let (l, r) = ctx.audio_out.channel_pair_mut(0, 1);
        for (l, r) in l.iter_mut().zip(r.iter_mut()) {
            let [size, _, _, predelay_ms, mix] = self.smoothers.each_mut().map(|s| s.next());
            let scale = MIN_SIZE + (1.0 - MIN_SIZE) * size.clamp(0.0, 1.0);
            let predelay = (predelay_ms * 0.001 * sr).max(1.0);
            let dry = [*l, *r];
            let mut input = [0.0; 2];
            for ch in 0..2 {
                self.predelay[ch].write(dry[ch]);
                let mut x = self.predelay[ch].read(predelay);
                for d in &mut self.diffusers[ch] {
                    x = d.process(x);
                }
                input[ch] = x;
            }

            let mut out = [0.0; LINES];
            for (i, line) in self.lines.iter().enumerate() {
                out[i] = line.read(LINE_MS[i] * 0.001 * scale * sr);
            }
            let mut back = [0.0; LINES];
            for i in 0..LINES {
                let s = out[i] + damping * (self.damped[i] - out[i]);
                self.damped[i] = if s.abs() < 1e-20 { 0.0 } else { s };
                back[i] = self.damped[i] * feedback[i];
            }
            // Householder: each line gets itself less a share of all,
            // which keeps the energy.
            let share = back.iter().sum::<f32>() * (2.0 / LINES as f32);
            for (i, line) in self.lines.iter_mut().enumerate() {
                line.write(back[i] - share + input[i % 2] * 0.5);
            }

            let wet_l: f32 = out.iter().zip(OUT_LEFT).map(|(o, s)| o * s).sum();
            let wet_r: f32 = out.iter().zip(OUT_RIGHT).map(|(o, s)| o * s).sum();
            let mix = mix.clamp(0.0, 1.0);
            *l = dry[0] * (1.0 - mix) + wet_l * WET_GAIN * mix;
            *r = dry[1] * (1.0 - mix) + wet_r * WET_GAIN * mix;
        }
    }

    fn tail_samples(&self) -> usize {
        let seconds = self.params[1].get() + REVERB_MAX_PREDELAY_MS * 0.001;
        (seconds * self.sample_rate) as usize
    }

    /// Keep the room ringing through edits: swap its buffers (no copy, no
    /// allocation), which are the same size at the same rate.
    fn take_state_from(&mut self, previous: &mut dyn Node) -> StateTransfer {
        if let Some(prev) = previous.as_any_mut().and_then(|a| a.downcast_mut::<Self>())
            && prev.sample_rate == self.sample_rate
        {
            std::mem::swap(&mut self.predelay, &mut prev.predelay);
            std::mem::swap(&mut self.diffusers, &mut prev.diffusers);
            std::mem::swap(&mut self.lines, &mut prev.lines);
            self.damped = prev.damped;
            self.smoothers = prev.smoothers.clone();
        }
        StateTransfer::KeepNew
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}
