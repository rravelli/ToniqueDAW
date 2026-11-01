//! Built-in effects: stateful nodes whose state must survive graph rebuilds.

use std::f32::consts::FRAC_1_SQRT_2;
use std::sync::Arc;

use crate::audio::AudioBuffer;
pub use crate::dsp::FilterMode;
use crate::dsp::{Biquad, BiquadState};
use crate::graph::{Node, NodeIdentity, NodeProperties, ProcessContext, StateTransfer};
use crate::param::{AtomicParam, Smoother};

/// Stereo RBJ biquad with smoothed frequency (Hz), Q and gain (dB, for
/// the shelves and peak).
pub struct FilterNode {
    mode: FilterMode,
    params: [Arc<AtomicParam>; 3],
    /// Cutoff, Q, gain.
    smoothers: [Smoother; 3],
    coefs: Biquad,
    state: [BiquadState; 2],
    sample_rate: f32,
    identity: Option<NodeIdentity>,
}

/// Samples between coefficient updates while a parameter ramps.
pub(crate) const COEF_UPDATE_INTERVAL: usize = 16;

impl FilterNode {
    pub fn new(
        mode: FilterMode,
        cutoff: Arc<AtomicParam>,
        q: Arc<AtomicParam>,
        gain_db: Arc<AtomicParam>,
    ) -> Self {
        let params = [cutoff, q, gain_db];
        Self {
            mode,
            smoothers: std::array::from_fn(|i| Smoother::new(params[i].get())),
            params,
            coefs: Biquad::default(),
            state: Default::default(),
            sample_rate: 48000.0,
            identity: None,
        }
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }

    fn update_coefs(&mut self, [cutoff, q, gain]: [f32; 3]) {
        self.coefs = Biquad::new(self.mode, cutoff, q, gain, self.sample_rate);
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
        for (s, p) in self.smoothers.iter_mut().zip(&self.params) {
            s.set_sample_rate(sample_rate);
            s.snap(p);
        }
        self.update_coefs(self.smoothers.each_ref().map(|s| s.current()));
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        ctx.sum_inputs_to_output();
        for (s, p) in self.smoothers.iter_mut().zip(&self.params) {
            s.retarget(p);
        }
        let len = ctx.block_len;
        let mut start = 0;
        while start < len {
            let end = (start + COEF_UPDATE_INTERVAL).min(len);
            // Advance smoothers per sample, recompute coefficients per chunk.
            if self.smoothers.iter().any(Smoother::is_ramping) {
                let mut values = [0.0; 3];
                for _ in start..end {
                    values = self.smoothers.each_mut().map(|s| s.next());
                }
                self.update_coefs(values);
            }
            let k = self.coefs;
            for (ch, st) in self.state.iter_mut().enumerate() {
                for x in &mut ctx.audio_out.channel_mut(ch)[start..end] {
                    *x = st.process(&k, *x);
                }
            }
            start = end;
        }
    }

    fn take_state_from(&mut self, previous: &mut dyn Node) -> StateTransfer {
        if let Some(prev) = previous.as_any_mut().and_then(|a| a.downcast_mut::<Self>()) {
            self.state = prev.state;
            self.smoothers = prev.smoothers.clone();
            // The mode may have changed: work the coefficients out again.
            self.sample_rate = prev.sample_rate;
            self.update_coefs(self.smoothers.each_ref().map(|s| s.current()));
        }
        StateTransfer::KeepNew
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

/// Stereo feedback delay ("echo"). Each repeat goes through a high- and a
/// low-pass, so repeats get thinner and darker as they fade. Its delay line
/// is exactly the kind of state that must be carried across graph rebuilds.
pub struct EchoNode {
    time_s: f32,
    /// Feedback, mix, low cut (Hz), high cut (Hz).
    params: [Arc<AtomicParam>; 4],
    smoothers: [Smoother; 4],
    /// High-pass then low-pass, per channel.
    cuts: [Biquad; 2],
    cut_state: [[BiquadState; 2]; 2],
    line: AudioBuffer,
    pos: usize,
    delay: usize,
    sample_rate: f32,
    identity: Option<NodeIdentity>,
}

impl EchoNode {
    pub fn new(
        time_s: f32,
        feedback: Arc<AtomicParam>,
        mix: Arc<AtomicParam>,
        low_cut: Arc<AtomicParam>,
        high_cut: Arc<AtomicParam>,
    ) -> Self {
        let params = [feedback, mix, low_cut, high_cut];
        Self {
            time_s,
            smoothers: std::array::from_fn(|i| Smoother::new(params[i].get())),
            params,
            cuts: [Biquad::THROUGH; 2],
            cut_state: Default::default(),
            line: AudioBuffer::new(0, 0),
            pos: 0,
            delay: 1,
            sample_rate: 48000.0,
            identity: None,
        }
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }

    fn update_cuts(&mut self) {
        let [_, _, low, high] = self.smoothers.each_ref().map(|s| s.current());
        self.cuts = [
            Biquad::new(
                FilterMode::HighPass,
                low,
                FRAC_1_SQRT_2,
                0.0,
                self.sample_rate,
            ),
            Biquad::new(
                FilterMode::LowPass,
                high,
                FRAC_1_SQRT_2,
                0.0,
                self.sample_rate,
            ),
        ];
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
        self.sample_rate = sample_rate as f32;
        self.delay = ((self.time_s as f64 * sample_rate) as usize).max(1);
        self.line = AudioBuffer::new(2, self.delay);
        for (s, p) in self.smoothers.iter_mut().zip(&self.params) {
            s.set_sample_rate(sample_rate);
            s.snap(p);
        }
        self.update_cuts();
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        ctx.sum_inputs_to_output();
        for (s, p) in self.smoothers.iter_mut().zip(&self.params) {
            s.retarget(p);
        }
        let (l, r) = ctx.audio_out.channel_pair_mut(0, 1);
        let mut p = self.pos;
        for (l, r) in l
            .chunks_mut(COEF_UPDATE_INTERVAL)
            .zip(r.chunks_mut(COEF_UPDATE_INTERVAL))
        {
            let ramping = self.smoothers[2..].iter().any(Smoother::is_ramping);
            for (l, r) in l.iter_mut().zip(r.iter_mut()) {
                let [fb, mix, ..] = self.smoothers.each_mut().map(|s| s.next());
                let fb = fb.clamp(0.0, 0.98);
                for (ch, s) in [l, r].into_iter().enumerate() {
                    let line = self.line.channel_mut(ch);
                    let [hp, lp] = &mut self.cut_state[ch];
                    let wet = lp.process(&self.cuts[1], hp.process(&self.cuts[0], line[p]));
                    line[p] = *s + wet * fb;
                    *s += wet * mix;
                }
                p = if p + 1 == self.delay { 0 } else { p + 1 };
            }
            if ramping {
                self.update_cuts();
            }
        }
        self.pos = p;
    }

    fn tail_samples(&self) -> usize {
        // Until the feedback has decayed by ~60 dB.
        let fb = self.params[0].get().clamp(0.01, 0.98);
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
            self.smoothers = prev.smoothers.clone();
            self.cuts = prev.cuts;
            self.cut_state = prev.cut_state;
        }
        StateTransfer::KeepNew
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

/// Gain below which a utility is silent, in dB: the bottom of its range.
pub const UTILITY_SILENT_DB: f32 = -60.0;

/// Stereo utility: width (0 mono, 1 as is, 2 twice as wide), a mono
/// switch, per-side phase inversion, balance, then gain in dB (silent at
/// [`UTILITY_SILENT_DB`]). Every control is ramped, switches included, so
/// nothing clicks.
pub struct UtilityNode {
    params: [Arc<AtomicParam>; 6],
    /// Gain (dB), balance, width, mono, invert left, invert right.
    smoothers: [Smoother; 6],
    identity: Option<NodeIdentity>,
}

impl UtilityNode {
    pub fn new(
        gain_db: Arc<AtomicParam>,
        balance: Arc<AtomicParam>,
        width: Arc<AtomicParam>,
        mono: Arc<AtomicParam>,
        invert_left: Arc<AtomicParam>,
        invert_right: Arc<AtomicParam>,
    ) -> Self {
        let params = [gain_db, balance, width, mono, invert_left, invert_right];
        Self {
            smoothers: std::array::from_fn(|i| Smoother::new(params[i].get())),
            params,
            identity: None,
        }
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }
}

impl Node for UtilityNode {
    fn properties(&self) -> NodeProperties {
        let p = NodeProperties::audio(2);
        match self.identity {
            Some(id) => p.with_identity(id),
            None => p,
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block: usize) {
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
        let (l, r) = ctx.audio_out.channel_pair_mut(0, 1);
        for (l, r) in l.iter_mut().zip(r.iter_mut()) {
            let [gain_db, balance, width, mono, invert_l, invert_r] =
                self.smoothers.each_mut().map(|s| s.next());
            let width = width * (1.0 - mono.clamp(0.0, 1.0));
            let mid = (*l + *r) * 0.5;
            let side = (*l - *r) * 0.5 * width;
            let gain = if gain_db <= UTILITY_SILENT_DB {
                0.0
            } else {
                10f32.powf(gain_db / 20.0)
            };
            // Balance: the far side fades out, the near one stays at unity.
            let balance = balance.clamp(-1.0, 1.0);
            let (gl, gr) = ((1.0 - balance).min(1.0), (1.0 + balance).min(1.0));
            *l = (mid + side) * gl * gain * (1.0 - 2.0 * invert_l.clamp(0.0, 1.0));
            *r = (mid - side) * gr * gain * (1.0 - 2.0 * invert_r.clamp(0.0, 1.0));
        }
    }

    /// Continue the ramps from where they were.
    fn take_state_from(&mut self, previous: &mut dyn Node) -> StateTransfer {
        if let Some(prev) = previous.as_any_mut().and_then(|a| a.downcast_mut::<Self>()) {
            self.smoothers = prev.smoothers.clone();
        }
        StateTransfer::KeepNew
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}
