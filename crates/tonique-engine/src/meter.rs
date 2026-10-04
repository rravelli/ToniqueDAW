//! Lock-free level metering and a short scope history, written by the RT
//! thread and read by the UI.
//!
//! The RT side only does relaxed atomic stores and one compare-exchange per
//! channel per block; it never allocates. Readers may see a scope window
//! that straddles two blocks, which is fine for display.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::audio::AudioBlock;

/// Samples of history kept per channel for the scope/spectrum view.
pub const SCOPE_LEN: usize = 2048;

const CHANNELS: usize = 2;

/// Peak and mean-square level of one channel since the last read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Level {
    pub peak: f32,
    pub rms: f32,
}

/// Levels and recent samples of a stereo channel strip.
pub struct ChannelMeter {
    peak: [AtomicU32; CHANNELS],
    sum_sq: [AtomicU32; CHANNELS],
    count: AtomicU32,
    scope: [Box<[AtomicU32]>; CHANNELS],
    scope_pos: AtomicUsize,
}

impl Default for ChannelMeter {
    fn default() -> Self {
        let scope = || (0..SCOPE_LEN).map(|_| AtomicU32::new(0)).collect();
        Self {
            peak: Default::default(),
            sum_sq: Default::default(),
            count: AtomicU32::new(0),
            scope: [scope(), scope()],
            scope_pos: AtomicUsize::new(0),
        }
    }
}

impl std::fmt::Debug for ChannelMeter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelMeter").finish_non_exhaustive()
    }
}

/// `f32` max/add on an `AtomicU32` holding the value's bits.
fn update_f32(a: &AtomicU32, f: impl Fn(f32) -> f32) {
    let _ = a.try_update(Ordering::Relaxed, Ordering::Relaxed, |bits| {
        Some(f(f32::from_bits(bits)).to_bits())
    });
}

impl ChannelMeter {
    /// RT side: account for one processed block. Allocation- and lock-free.
    pub fn record(&self, block: &AudioBlock) {
        let n = block.len();
        if n == 0 || block.channels() == 0 {
            return;
        }
        let start = self.scope_pos.load(Ordering::Relaxed);
        for ch in 0..CHANNELS {
            let data = block.channel(ch.min(block.channels() - 1));
            let (mut peak, mut sum) = (0.0f32, 0.0f32);
            for (i, &s) in data.iter().enumerate() {
                peak = peak.max(s.abs());
                sum += s * s;
                self.scope[ch][(start + i) % SCOPE_LEN].store(s.to_bits(), Ordering::Relaxed);
            }
            update_f32(&self.peak[ch], |p| p.max(peak));
            update_f32(&self.sum_sq[ch], |s| s + sum);
        }
        self.count.fetch_add(n as u32, Ordering::Relaxed);
        self.scope_pos
            .store((start + n) % SCOPE_LEN, Ordering::Relaxed);
    }

    /// UI side: levels since the previous call, then reset. `None` if no
    /// audio was processed in between (the UI can redraw faster than that).
    pub fn take_levels(&self) -> Option<[Level; CHANNELS]> {
        let count = self.count.swap(0, Ordering::Relaxed);
        if count == 0 {
            return None;
        }
        Some(std::array::from_fn(|ch| {
            let peak = f32::from_bits(self.peak[ch].swap(0, Ordering::Relaxed));
            let sum = f32::from_bits(self.sum_sq[ch].swap(0, Ordering::Relaxed));
            Level {
                peak,
                rms: (sum / count as f32).sqrt(),
            }
        }))
    }

    /// UI side: the most recent `out.len()` samples (at most [`SCOPE_LEN`])
    /// of `channel`, oldest first.
    pub fn read_scope(&self, channel: usize, out: &mut [f32]) {
        let n = out.len().min(SCOPE_LEN);
        let end = self.scope_pos.load(Ordering::Relaxed);
        let ring = &self.scope[channel.min(CHANNELS - 1)];
        for (i, o) in out[..n].iter_mut().enumerate() {
            *o =
                f32::from_bits(ring[(end + SCOPE_LEN - n + i) % SCOPE_LEN].load(Ordering::Relaxed));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::AudioBuffer;

    #[test]
    fn levels_accumulate_until_read() {
        let meter = ChannelMeter::default();
        let mut buf = AudioBuffer::new(2, 4);
        buf.channel_mut(0).copy_from_slice(&[0.5, -1.0, 0.0, 0.0]);
        buf.channel_mut(1).copy_from_slice(&[0.25; 4]);
        meter.record(&buf.block(4));
        meter.record(&buf.block(4));

        let [l, r] = meter.take_levels().unwrap();
        assert_eq!(l.peak, 1.0);
        assert!((l.rms - (1.25f32 / 4.0).sqrt()).abs() < 1e-6);
        assert!((r.rms - 0.25).abs() < 1e-6);
        assert_eq!(meter.take_levels(), None);
    }

    #[test]
    fn scope_returns_latest_samples_in_order() {
        let meter = ChannelMeter::default();
        let mut buf = AudioBuffer::new(1, SCOPE_LEN);
        for (i, s) in buf.channel_mut(0).iter_mut().enumerate() {
            *s = i as f32;
        }
        meter.record(&buf.block(SCOPE_LEN));
        meter.record(&buf.block(3));

        let mut out = [0.0; 4];
        meter.read_scope(1, &mut out); // mono input feeds both channels
        assert_eq!(out, [(SCOPE_LEN - 1) as f32, 0.0, 1.0, 2.0]);
    }
}
