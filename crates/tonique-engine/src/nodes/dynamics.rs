//! Dynamics: compressor, limiter and gate. Each reports the gain it takes
//! off to its plugin's [`ReductionMeter`](crate::meter::ReductionMeter).

use std::sync::Arc;

use crate::dsp::{db_to_gain, gain_to_db, time_coef};
use crate::graph::{Node, NodeIdentity, NodeProperties, ProcessContext, StateTransfer};
use crate::meter::PluginTap;
use crate::param::{AtomicParam, Smoother};

/// Gain reduction of a compressor for a level `over` its threshold (dB,
/// negative below it), as a positive number of dB. The knee rounds the
/// corner over `knee` dB centred on the threshold.
pub fn compressor_reduction(over: f32, ratio: f32, knee: f32) -> f32 {
    let slope = 1.0 - 1.0 / ratio.max(1.0);
    if knee > 0.0 && over.abs() <= knee / 2.0 {
        slope * (over + knee / 2.0).powi(2) / (2.0 * knee)
    } else if over > 0.0 {
        slope * over
    } else {
        0.0
    }
}

/// Smooths a gain reduction (dB): rises at the attack's pace, falls at the
/// release's.
#[derive(Clone, Copy, Debug, Default)]
struct Ballistics {
    attack: f32,
    release: f32,
    value: f32,
}

impl Ballistics {
    fn set_times(&mut self, attack_ms: f32, release_ms: f32, sample_rate: f32) {
        self.attack = time_coef(attack_ms, sample_rate);
        self.release = time_coef(release_ms, sample_rate);
    }

    #[inline]
    fn next(&mut self, target: f32) -> f32 {
        let coef = if target > self.value {
            self.attack
        } else {
            self.release
        };
        self.value = target + coef * (self.value - target);
        self.value
    }
}

/// Smoothers for a node's parameters, kept in step with them.
#[derive(Clone)]
struct Params<const N: usize> {
    params: [Arc<AtomicParam>; N],
    smoothers: [Smoother; N],
}

impl<const N: usize> Params<N> {
    fn new(params: [Arc<AtomicParam>; N]) -> Self {
        Self {
            smoothers: std::array::from_fn(|i| Smoother::new(params[i].get())),
            params,
        }
    }

    fn prepare(&mut self, sample_rate: f64) {
        for (s, p) in self.smoothers.iter_mut().zip(&self.params) {
            s.set_sample_rate(sample_rate);
            s.snap(p);
        }
    }

    fn retarget(&mut self) {
        for (s, p) in self.smoothers.iter_mut().zip(&self.params) {
            s.retarget(p);
        }
    }

    #[inline]
    fn next(&mut self) -> [f32; N] {
        self.smoothers.each_mut().map(|s| s.next())
    }

    fn current(&self) -> [f32; N] {
        self.smoothers.each_ref().map(|s| s.current())
    }

    /// Carry the ramps over from `previous`.
    fn take_from(&mut self, previous: &Self) {
        self.smoothers = previous.smoothers.clone();
    }
}

fn with_identity(p: NodeProperties, id: Option<NodeIdentity>) -> NodeProperties {
    match id {
        Some(id) => p.with_identity(id),
        None => p,
    }
}

/// Stereo-linked feed-forward compressor: threshold (dB), ratio, attack
/// (ms), release (ms), knee (dB) and makeup gain (dB).
pub struct CompressorNode {
    params: Params<6>,
    envelope: Ballistics,
    tap: Arc<PluginTap>,
    sample_rate: f32,
    identity: Option<NodeIdentity>,
}

impl CompressorNode {
    pub fn new(
        [threshold, ratio, attack, release, knee, makeup]: [Arc<AtomicParam>; 6],
        tap: Arc<PluginTap>,
    ) -> Self {
        Self {
            params: Params::new([threshold, ratio, attack, release, knee, makeup]),
            envelope: Ballistics::default(),
            tap,
            sample_rate: 48000.0,
            identity: None,
        }
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }
}

impl Node for CompressorNode {
    fn properties(&self) -> NodeProperties {
        with_identity(NodeProperties::audio(2), self.identity)
    }

    fn prepare(&mut self, sample_rate: f64, _max_block: usize) {
        self.sample_rate = sample_rate as f32;
        self.params.prepare(sample_rate);
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        ctx.sum_inputs_to_output();
        self.params.retarget();
        let [_, _, attack, release, ..] = self.params.current();
        self.envelope.set_times(attack, release, self.sample_rate);
        let mut most = 0f32;
        let (l, r) = ctx.audio_out.channel_pair_mut(0, 1);
        for (l, r) in l.iter_mut().zip(r.iter_mut()) {
            let [threshold, ratio, _, _, knee, makeup] = self.params.next();
            let level = gain_to_db(l.abs().max(r.abs()));
            let reduction =
                self.envelope
                    .next(compressor_reduction(level - threshold, ratio, knee));
            most = most.max(reduction);
            let gain = db_to_gain(makeup - reduction);
            *l *= gain;
            *r *= gain;
        }
        self.tap.reduction.record(most);
    }

    fn take_state_from(&mut self, previous: &mut dyn Node) -> StateTransfer {
        if let Some(prev) = previous.as_any_mut().and_then(|a| a.downcast_mut::<Self>()) {
            self.params.take_from(&prev.params);
            self.envelope = prev.envelope;
        }
        StateTransfer::KeepNew
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

/// The smallest of the last `window` values pushed, in constant time per
/// value (a monotonic queue). Sized once.
#[derive(Clone)]
struct SlidingMin {
    /// (index pushed at, value), increasing values from `head`.
    ring: Vec<(u64, f32)>,
    head: usize,
    len: usize,
    window: u64,
    pushed: u64,
}

impl SlidingMin {
    fn new(window: usize) -> Self {
        Self {
            ring: vec![(0, 0.0); window + 1],
            head: 0,
            len: 0,
            window: window as u64,
            pushed: 0,
        }
    }

    #[inline]
    fn push(&mut self, value: f32) -> f32 {
        let cap = self.ring.len();
        while self.len > 0 && self.ring[(self.head + self.len - 1) % cap].1 >= value {
            self.len -= 1;
        }
        self.ring[(self.head + self.len) % cap] = (self.pushed, value);
        self.len += 1;
        self.pushed += 1;
        while self.ring[self.head].0 + self.window < self.pushed {
            self.head = (self.head + 1) % cap;
            self.len -= 1;
        }
        self.ring[self.head].1
    }
}

/// Lookahead brickwall limiter: input gain (dB), ceiling (dB) and release
/// (ms). It sees peaks coming `lookahead` samples early and turns down
/// ahead of them, so its output never goes over the ceiling; that delays
/// the audio by as much, which it reports as latency.
pub struct LimiterNode {
    params: Params<3>,
    lookahead: usize,
    /// The gain each sample needs, smallest over the lookahead.
    needed: SlidingMin,
    /// Gain after release, averaged over the lookahead: ramps down ahead
    /// of a peak rather than jumping.
    ramp: Vec<f32>,
    ramp_pos: usize,
    ramp_sum: f64,
    released: f32,
    release_coef: f32,
    /// The audio, delayed by the lookahead.
    delay: [Vec<f32>; 2],
    delay_pos: usize,
    tap: Arc<PluginTap>,
    sample_rate: f32,
    identity: Option<NodeIdentity>,
}

impl LimiterNode {
    /// `lookahead` is in samples, at least 1.
    pub fn new(
        lookahead: usize,
        [gain, ceiling, release]: [Arc<AtomicParam>; 3],
        tap: Arc<PluginTap>,
    ) -> Self {
        let lookahead = lookahead.max(1);
        Self {
            params: Params::new([gain, ceiling, release]),
            lookahead,
            needed: SlidingMin::new(lookahead + 1),
            ramp: vec![1.0; lookahead],
            ramp_pos: 0,
            ramp_sum: lookahead as f64,
            released: 1.0,
            release_coef: 0.0,
            delay: [vec![0.0; lookahead], vec![0.0; lookahead]],
            delay_pos: 0,
            tap,
            sample_rate: 48000.0,
            identity: None,
        }
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }
}

impl Node for LimiterNode {
    fn properties(&self) -> NodeProperties {
        with_identity(NodeProperties::audio(2), self.identity).with_latency(self.lookahead)
    }

    fn prepare(&mut self, sample_rate: f64, _max_block: usize) {
        self.sample_rate = sample_rate as f32;
        self.params.prepare(sample_rate);
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        ctx.sum_inputs_to_output();
        self.params.retarget();
        let [.., release] = self.params.current();
        // From the gain's floor back up to unity in about the release.
        self.release_coef = 1.0 - time_coef(release, self.sample_rate);
        let n = self.lookahead;
        let mut least = 1f32;
        let (l, r) = ctx.audio_out.channel_pair_mut(0, 1);
        for (l, r) in l.iter_mut().zip(r.iter_mut()) {
            let [gain_db, ceiling_db, _] = self.params.next();
            let (gain, ceiling) = (db_to_gain(gain_db), db_to_gain(ceiling_db));
            let (x_l, x_r) = (*l * gain, *r * gain);
            let peak = x_l.abs().max(x_r.abs());
            let needed = if peak > ceiling { ceiling / peak } else { 1.0 };
            // Every value averaged below was at most the gain this
            // sample needs, from n samples before it to now: their
            // average is too, by the time it comes out of the delay.
            let floor = self.needed.push(needed);
            self.released += (1.0 - self.released) * self.release_coef;
            self.released = self.released.min(floor);
            self.ramp_sum += self.released as f64 - self.ramp[self.ramp_pos] as f64;
            self.ramp[self.ramp_pos] = self.released;
            self.ramp_pos = (self.ramp_pos + 1) % n;
            let applied = (self.ramp_sum / n as f64) as f32;
            least = least.min(applied);

            let p = self.delay_pos;
            let (d_l, d_r) = (self.delay[0][p], self.delay[1][p]);
            self.delay[0][p] = x_l;
            self.delay[1][p] = x_r;
            self.delay_pos = (p + 1) % n;
            // Rounding in the running sum can't push it over.
            *l = (d_l * applied).clamp(-ceiling, ceiling);
            *r = (d_r * applied).clamp(-ceiling, ceiling);
        }
        self.tap.reduction.record(-gain_to_db(least));
    }

    fn tail_samples(&self) -> usize {
        self.lookahead
    }

    fn take_state_from(&mut self, previous: &mut dyn Node) -> StateTransfer {
        if let Some(prev) = previous.as_any_mut().and_then(|a| a.downcast_mut::<Self>()) {
            self.params.take_from(&prev.params);
            // The audio in flight only fits if the lookahead is the same.
            if prev.lookahead == self.lookahead {
                std::mem::swap(&mut self.needed, &mut prev.needed);
                std::mem::swap(&mut self.ramp, &mut prev.ramp);
                std::mem::swap(&mut self.delay, &mut prev.delay);
                self.ramp_pos = prev.ramp_pos;
                self.ramp_sum = prev.ramp_sum;
                self.delay_pos = prev.delay_pos;
                self.released = prev.released;
            }
        }
        StateTransfer::KeepNew
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

/// Gate: below the threshold (dB), turns the audio down by the range (dB;
/// all the way at its bottom, a little for a gentle expander). Opens over
/// the attack (ms), stays open for the hold (ms) once the audio drops,
/// then closes over the release (ms).
pub struct GateNode {
    params: Params<5>,
    /// Gain applied, 1 when open.
    gain: f32,
    /// Samples left before closing.
    holding: usize,
    open: bool,
    tap: Arc<PluginTap>,
    sample_rate: f32,
    identity: Option<NodeIdentity>,
}

/// Below the bottom of a gate's range, in dB, it's silent.
pub const GATE_SILENT_DB: f32 = -80.0;
/// How far below its threshold a gate closes again, in dB, so a level
/// hovering at the threshold doesn't chatter.
const GATE_HYSTERESIS_DB: f32 = 3.0;

impl GateNode {
    pub fn new(
        [threshold, range, attack, hold, release]: [Arc<AtomicParam>; 5],
        tap: Arc<PluginTap>,
    ) -> Self {
        Self {
            params: Params::new([threshold, range, attack, hold, release]),
            gain: 1.0,
            holding: 0,
            open: true,
            tap,
            sample_rate: 48000.0,
            identity: None,
        }
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }
}

impl Node for GateNode {
    fn properties(&self) -> NodeProperties {
        with_identity(NodeProperties::audio(2), self.identity)
    }

    fn prepare(&mut self, sample_rate: f64, _max_block: usize) {
        self.sample_rate = sample_rate as f32;
        self.params.prepare(sample_rate);
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        ctx.sum_inputs_to_output();
        self.params.retarget();
        let [_, _, attack, hold, release] = self.params.current();
        let attack = time_coef(attack, self.sample_rate);
        let release = time_coef(release, self.sample_rate);
        let hold = (hold * 0.001 * self.sample_rate) as usize;
        let mut least = 1f32;
        let (l, r) = ctx.audio_out.channel_pair_mut(0, 1);
        for (l, r) in l.iter_mut().zip(r.iter_mut()) {
            let [threshold, range, ..] = self.params.next();
            let level = gain_to_db(l.abs().max(r.abs()));
            if level > threshold {
                self.open = true;
                self.holding = hold;
            } else if level < threshold - GATE_HYSTERESIS_DB {
                if self.holding > 0 {
                    self.holding -= 1;
                } else {
                    self.open = false;
                }
            }
            let floor = if range <= GATE_SILENT_DB {
                0.0
            } else {
                db_to_gain(range)
            };
            let (target, coef) = if self.open {
                (1.0, attack)
            } else {
                (floor, release)
            };
            self.gain = target + coef * (self.gain - target);
            least = least.min(self.gain);
            *l *= self.gain;
            *r *= self.gain;
        }
        self.tap.reduction.record(-gain_to_db(least));
    }

    fn take_state_from(&mut self, previous: &mut dyn Node) -> StateTransfer {
        if let Some(prev) = previous.as_any_mut().and_then(|a| a.downcast_mut::<Self>()) {
            self.params.take_from(&prev.params);
            self.gain = prev.gain;
            self.holding = prev.holding;
            self.open = prev.open;
        }
        StateTransfer::KeepNew
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduction_follows_the_ratio_and_rounds_the_knee() {
        assert_eq!(compressor_reduction(-10.0, 4.0, 0.0), 0.0);
        assert_eq!(compressor_reduction(12.0, 4.0, 0.0), 9.0);
        assert_eq!(compressor_reduction(12.0, 1.0, 6.0), 0.0);
        // The knee starts reducing below the threshold, and joins the line
        // smoothly.
        assert!(compressor_reduction(-2.0, 4.0, 6.0) > 0.0);
        assert_eq!(compressor_reduction(-3.0, 4.0, 6.0), 0.0);
        assert_eq!(compressor_reduction(0.0, 4.0, 6.0), 0.75 * 9.0 / 12.0);
        // Joins the line at the knee's edge.
        let edge = compressor_reduction(3.0, 4.0, 6.0);
        assert!((edge - 2.25).abs() < 1e-5, "{edge}");
    }

    #[test]
    fn sliding_min_forgets_old_values() {
        let mut m = SlidingMin::new(3);
        let got: Vec<f32> = [5.0, 1.0, 4.0, 4.0, 4.0, 2.0, 3.0]
            .into_iter()
            .map(|v| m.push(v))
            .collect();
        assert_eq!(got, [5.0, 1.0, 1.0, 1.0, 4.0, 2.0, 2.0]);
    }
}
