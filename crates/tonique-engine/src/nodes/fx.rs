//! Built-in effects: stateful nodes whose state must survive graph rebuilds.

use std::f32::consts::PI;
use std::sync::Arc;

use crate::audio::AudioBuffer;
use crate::graph::{Node, NodeIdentity, NodeProperties, ProcessContext, StateTransfer};
use crate::param::{AtomicParam, Smoother};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FilterMode {
    LowPass,
    HighPass,
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

/// Stereo RBJ biquad with smoothed cutoff (Hz) and Q.
pub struct FilterNode {
    mode: FilterMode,
    cutoff: Arc<AtomicParam>,
    q: Arc<AtomicParam>,
    cutoff_s: Smoother,
    q_s: Smoother,
    coefs: Biquad,
    /// Direct form I state per channel: x1, x2, y1, y2.
    state: [[f32; 4]; 2],
    sample_rate: f32,
    identity: Option<NodeIdentity>,
}

const COEF_UPDATE_INTERVAL: usize = 16;

impl FilterNode {
    pub fn new(mode: FilterMode, cutoff: Arc<AtomicParam>, q: Arc<AtomicParam>) -> Self {
        Self {
            mode,
            cutoff_s: Smoother::new(cutoff.get()),
            q_s: Smoother::new(q.get()),
            cutoff,
            q,
            coefs: Biquad::default(),
            state: [[0.0; 4]; 2],
            sample_rate: 48000.0,
            identity: None,
        }
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }

    fn update_coefs(&mut self, cutoff: f32, q: f32) {
        let f = cutoff.clamp(10.0, self.sample_rate * 0.49);
        let w = 2.0 * PI * f / self.sample_rate;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q.max(0.05));
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = match self.mode {
            FilterMode::LowPass => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0),
            FilterMode::HighPass => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0),
        };
        self.coefs = Biquad { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: -2.0 * cos / a0, a2: (1.0 - alpha) / a0 };
    }
}

impl Node for FilterNode {
    fn properties(&self) -> NodeProperties {
        let p = NodeProperties::audio(2);
        match self.identity {
            Some(id) => p.with_identity(id),
            None => p,
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block: usize) {
        self.sample_rate = sample_rate as f32;
        for (s, p) in [(&mut self.cutoff_s, &self.cutoff), (&mut self.q_s, &self.q)] {
            s.set_sample_rate(sample_rate);
            s.snap(p);
        }
        self.update_coefs(self.cutoff_s.current(), self.q_s.current());
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        ctx.sum_inputs_to_output();
        self.cutoff_s.retarget(&self.cutoff);
        self.q_s.retarget(&self.q);
        let len = ctx.block_len;
        let mut start = 0;
        while start < len {
            let end = (start + COEF_UPDATE_INTERVAL).min(len);
            // Advance smoothers per sample, recompute coefficients per chunk.
            let (mut c, mut q) = (0.0, 0.0);
            for _ in start..end {
                c = self.cutoff_s.next();
                q = self.q_s.next();
            }
            self.update_coefs(c, q);
            let k = self.coefs;
            for ch in 0..2 {
                let st = &mut self.state[ch];
                for x in &mut ctx.audio_out.channel_mut(ch)[start..end] {
                    let y = k.b0 * *x + k.b1 * st[0] + k.b2 * st[1] - k.a1 * st[2] - k.a2 * st[3];
                    st[1] = st[0];
                    st[0] = *x;
                    st[3] = st[2];
                    st[2] = y;
                    *x = y;
                }
            }
            start = end;
        }
    }

    fn take_state_from(&mut self, previous: &mut dyn Node) -> StateTransfer {
        if let Some(prev) = previous.as_any_mut().and_then(|a| a.downcast_mut::<Self>()) {
            self.state = prev.state;
            self.cutoff_s = prev.cutoff_s.clone();
            self.q_s = prev.q_s.clone();
            self.coefs = prev.coefs;
        }
        StateTransfer::KeepNew
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

/// Stereo feedback delay ("echo"). Its delay line is exactly the kind of
/// state that must be carried across graph rebuilds.
pub struct EchoNode {
    time_s: f32,
    feedback: Arc<AtomicParam>,
    mix: Arc<AtomicParam>,
    fb_s: Smoother,
    mix_s: Smoother,
    line: AudioBuffer,
    pos: usize,
    delay: usize,
    identity: Option<NodeIdentity>,
}

impl EchoNode {
    pub fn new(time_s: f32, feedback: Arc<AtomicParam>, mix: Arc<AtomicParam>) -> Self {
        Self {
            time_s,
            fb_s: Smoother::new(feedback.get()),
            mix_s: Smoother::new(mix.get()),
            feedback,
            mix,
            line: AudioBuffer::new(0, 0),
            pos: 0,
            delay: 1,
            identity: None,
        }
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }
}

impl Node for EchoNode {
    fn properties(&self) -> NodeProperties {
        let p = NodeProperties::audio(2);
        match self.identity {
            Some(id) => p.with_identity(id),
            None => p,
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block: usize) {
        self.delay = ((self.time_s as f64 * sample_rate) as usize).max(1);
        self.line = AudioBuffer::new(2, self.delay);
        for (s, p) in [(&mut self.fb_s, &self.feedback), (&mut self.mix_s, &self.mix)] {
            s.set_sample_rate(sample_rate);
            s.snap(p);
        }
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        ctx.sum_inputs_to_output();
        self.fb_s.retarget(&self.feedback);
        self.mix_s.retarget(&self.mix);
        let (l, r) = ctx.audio_out.channel_pair_mut(0, 1);
        let mut p = self.pos;
        for (l, r) in l.iter_mut().zip(r.iter_mut()) {
            let fb = self.fb_s.next().clamp(0.0, 0.98);
            let mix = self.mix_s.next();
            for (ch, s) in [l, r].into_iter().enumerate() {
                let line = self.line.channel_mut(ch);
                let wet = line[p];
                line[p] = *s + wet * fb;
                *s += wet * mix;
            }
            p = if p + 1 == self.delay { 0 } else { p + 1 };
        }
        self.pos = p;
    }

    fn tail_samples(&self) -> usize {
        // Until the feedback has decayed by ~60 dB.
        let fb = self.feedback.get().clamp(0.01, 0.98);
        let repeats = (-6.9 / fb.ln()).ceil() as usize;
        self.delay * (repeats + 1)
    }

    /// Swap delay lines (no copy, no allocation) so echoes keep ringing
    /// through edits. Only possible when the line length is unchanged.
    fn take_state_from(&mut self, previous: &mut dyn Node) -> StateTransfer {
        if let Some(prev) = previous.as_any_mut().and_then(|a| a.downcast_mut::<Self>())
            && prev.delay == self.delay
        {
            std::mem::swap(&mut self.line, &mut prev.line);
            self.pos = prev.pos;
            self.fb_s = prev.fb_s.clone();
            self.mix_s = prev.mix_s.clone();
        }
        StateTransfer::KeepNew
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}
