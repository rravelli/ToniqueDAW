//! Lock-free parameters and RT-side smoothing.

use std::sync::atomic::{AtomicU64, Ordering};

/// Stable identifier for an automatable parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ParamId(pub u64);

/// A parameter target shared between any control thread and the RT thread.
///
/// Target value and ramp time are packed into one 64-bit atomic so a reader
/// can never observe a value from one `set` paired with the ramp of another.
/// The current (smoothed) value lives in a [`Smoother`] owned by the RT side.
#[derive(Debug)]
pub struct AtomicParam {
    packed: AtomicU64,
}

fn pack(value: f32, ramp_ms: f32) -> u64 {
    (value.to_bits() as u64) << 32 | ramp_ms.max(0.0).to_bits() as u64
}

fn unpack(bits: u64) -> (f32, f32) {
    (
        f32::from_bits((bits >> 32) as u32),
        f32::from_bits(bits as u32),
    )
}

impl AtomicParam {
    pub fn new(value: f32) -> Self {
        Self {
            packed: AtomicU64::new(pack(value, 0.0)),
        }
    }

    /// Callable from any thread (UI, automation, MIDI learn). The RT side
    /// ramps from wherever it currently is to `value` over `ramp_ms`.
    pub fn set(&self, value: f32, ramp_ms: f32) {
        self.packed.store(pack(value, ramp_ms), Ordering::Relaxed);
    }

    /// The most recently requested target value.
    pub fn get(&self) -> f32 {
        unpack(self.packed.load(Ordering::Relaxed)).0
    }

    fn load_packed(&self) -> u64 {
        self.packed.load(Ordering::Relaxed)
    }
}

/// Per-sample linear ramp towards an [`AtomicParam`]'s target. Owned
/// exclusively by the RT thread.
#[derive(Clone, Debug)]
pub struct Smoother {
    current: f32,
    step: f32,
    remaining: u32,
    target: f32,
    /// Last packed (value, ramp) seen; retargeting only happens on change so
    /// a long ramp isn't restarted every block.
    seen: u64,
    sample_rate: f64,
}

impl Smoother {
    pub fn new(initial: f32) -> Self {
        Self {
            current: initial,
            step: 0.0,
            remaining: 0,
            target: initial,
            seen: u64::MAX,
            sample_rate: 48000.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
    }

    /// Jump straight to the param's current target (used on prepare).
    pub fn snap(&mut self, param: &AtomicParam) {
        let bits = param.load_packed();
        self.seen = bits;
        self.current = unpack(bits).0;
        self.target = self.current;
        self.remaining = 0;
    }

    /// Pick up a new target if one was set. Call once at the top of a block.
    pub fn retarget(&mut self, param: &AtomicParam) {
        let bits = param.load_packed();
        if bits == self.seen {
            return;
        }
        self.seen = bits;
        let (target, ramp_ms) = unpack(bits);
        let n = (ramp_ms as f64 * 0.001 * self.sample_rate).round().max(1.0) as u32;
        self.target = target;
        self.step = (target - self.current) / n as f32;
        self.remaining = n;
    }

    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f32 {
        if self.remaining > 0 {
            self.remaining -= 1;
            // Land exactly on the target to avoid float drift.
            self.current = if self.remaining == 0 {
                self.target
            } else {
                self.current + self.step
            };
        }
        self.current
    }

    pub fn current(&self) -> f32 {
        self.current
    }

    pub fn is_ramping(&self) -> bool {
        self.remaining > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramps_linearly_and_lands_on_target() {
        let p = AtomicParam::new(0.0);
        let mut s = Smoother::new(0.0);
        s.set_sample_rate(1000.0);
        s.snap(&p);
        p.set(1.0, 4.0); // 4 samples at 1 kHz
        s.retarget(&p);
        let v: Vec<f32> = (0..6).map(|_| s.next()).collect();
        assert_eq!(v, vec![0.25, 0.5, 0.75, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn retarget_without_change_does_not_restart_ramp() {
        let p = AtomicParam::new(0.0);
        let mut s = Smoother::new(0.0);
        s.set_sample_rate(1000.0);
        s.snap(&p);
        p.set(1.0, 10.0);
        s.retarget(&p);
        for _ in 0..5 {
            s.next();
        }
        s.retarget(&p); // same target: keeps going
        for _ in 0..5 {
            s.next();
        }
        assert_eq!(s.current(), 1.0);
        assert!(!s.is_ramping());
    }
}
