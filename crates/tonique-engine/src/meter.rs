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
        Self::with_scope_len(SCOPE_LEN)
    }
}

impl ChannelMeter {
    /// A meter keeping `len` samples of history per channel rather than
    /// [`SCOPE_LEN`], e.g. for a finer spectrum.
    pub fn with_scope_len(len: usize) -> Self {
        let scope = || (0..len.max(1)).map(|_| AtomicU32::new(0)).collect();
        Self {
            peak: Default::default(),
            sum_sq: Default::default(),
            count: AtomicU32::new(0),
            scope: [scope(), scope()],
            scope_pos: AtomicUsize::new(0),
        }
    }

    /// Samples of history kept per channel.
    pub fn scope_len(&self) -> usize {
        self.scope[0].len()
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
        let len = self.scope_len();
        for ch in 0..CHANNELS {
            let data = block.channel(ch.min(block.channels() - 1));
            let (mut peak, mut sum) = (0.0f32, 0.0f32);
            for (i, &s) in data.iter().enumerate() {
                peak = peak.max(s.abs());
                sum += s * s;
                self.scope[ch][(start + i) % len].store(s.to_bits(), Ordering::Relaxed);
            }
            update_f32(&self.peak[ch], |p| p.max(peak));
            update_f32(&self.sum_sq[ch], |s| s + sum);
        }
        self.count.fetch_add(n as u32, Ordering::Relaxed);
        self.scope_pos.store((start + n) % len, Ordering::Relaxed);
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

    /// UI side: the most recent `out.len()` samples (at most
    /// [`Self::scope_len`]) of `channel`, oldest first.
    pub fn read_scope(&self, channel: usize, out: &mut [f32]) {
        let len = self.scope_len();
        let n = out.len().min(len);
        let end = self.scope_pos.load(Ordering::Relaxed);
        let ring = &self.scope[channel.min(CHANNELS - 1)];
        for (i, o) in out[..n].iter_mut().enumerate() {
            *o = f32::from_bits(ring[(end + len - n + i) % len].load(Ordering::Relaxed));
        }
    }
}

/// History kept by a [`PluginTap`]: enough for a fine spectrum.
pub const TAP_SCOPE_LEN: usize = 8192;

/// The audio going into and out of a plugin, for its editor to show. Each
/// side only records while watched (see [`Self::watch_input_for`]), so
/// plugins nobody looks at cost nothing.
#[derive(Debug)]
pub struct PluginTap {
    pub input: ChannelMeter,
    pub output: ChannelMeter,
    /// Frames left to record, input then output.
    watched: [AtomicU32; 2],
}

impl Default for PluginTap {
    fn default() -> Self {
        Self {
            input: ChannelMeter::with_scope_len(TAP_SCOPE_LEN),
            output: ChannelMeter::with_scope_len(TAP_SCOPE_LEN),
            watched: Default::default(),
        }
    }
}

impl PluginTap {
    /// UI side: record the input for the next `frames`. Call it again
    /// while shown: it stops by itself once nobody does.
    pub fn watch_input_for(&self, frames: u32) {
        self.watched[0].fetch_max(frames, Ordering::Relaxed);
    }

    /// UI side: as [`Self::watch_input_for`], for the output.
    pub fn watch_output_for(&self, frames: u32) {
        self.watched[1].fetch_max(frames, Ordering::Relaxed);
    }

    /// RT side: whether to record the input and the output of a block of
    /// `frames`, counting it.
    pub fn take_block(&self, frames: usize) -> [bool; 2] {
        self.watched.each_ref().map(|left| {
            left.try_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                (left > 0).then(|| left.saturating_sub(frames as u32))
            })
            .is_ok()
        })
    }
}
