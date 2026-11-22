//! Audio from an input device, on its way into the graph.
//!
//! ```text
//!   input callback ── InputWriter ─ SPSC ring ─ InputFeed ──▶ AudioProcessor
//!                                                             │ per chunk: pull
//!                                                             ▼
//!                            BlockInfo::input ─▶ ProcessContext::device_input
//! ```
//!
//! The output callback is the clock: it takes one frame from the ring for
//! each frame it renders, so every input frame has a known place. The ring
//! sits at a target fill to absorb both callbacks' jitter, and goes back to
//! it after a problem, so the delay between input and output doesn't change:
//!
//! - **Underrun** (the ring runs dry): silence is output until the ring is
//!   back at its target.
//! - **Overflow** (the output stalled, so the ring is too full): the oldest
//!   frames are dropped down to the target.
//!
//! Both are counted in frames in [`InputStatus`].

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::meter::update_f32;

/// Which input channels feed a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InputRoute {
    /// One channel, sent to both sides.
    Mono(usize),
    /// A pair: this channel on the left, the next one on the right.
    Stereo(usize),
}

/// State of an input, published for the control side to read.
#[derive(Debug)]
pub struct InputStatus {
    /// Per channel, as `f32` bits.
    peaks: Box<[AtomicU32]>,
    underruns: AtomicU64,
    overflows: AtomicU64,
    latency: AtomicUsize,
    fill: AtomicUsize,
}

impl InputStatus {
    fn new(channels: usize) -> Self {
        Self {
            peaks: (0..channels).map(|_| AtomicU32::new(0)).collect(),
            underruns: AtomicU64::new(0),
            overflows: AtomicU64::new(0),
            latency: AtomicUsize::new(0),
            fill: AtomicUsize::new(0),
        }
    }

    pub fn channels(&self) -> usize {
        self.peaks.len()
    }

    /// Absolute peak of `channel` since the last call; 0 if it doesn't exist.
    pub fn take_peak(&self, channel: usize) -> f32 {
        self.peaks.get(channel).map_or(0.0, |peak| {
            f32::from_bits(peak.swap(0.0f32.to_bits(), Ordering::Relaxed))
        })
    }

    /// Frames of silence output since the input first delivered audio.
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    /// Frames dropped because the ring was too full. This grows while no
    /// engine is pulling the feed, so compare two readings over the time
    /// that matters rather than expecting 0.
    pub fn overflows(&self) -> u64 {
        self.overflows.load(Ordering::Relaxed)
    }

    /// Frames from the device capturing audio to its callback delivering it.
    pub fn latency_frames(&self) -> usize {
        self.latency.load(Ordering::Relaxed)
    }

    /// Frames left in the ring after the last pull.
    pub fn fill_frames(&self) -> usize {
        self.fill.load(Ordering::Relaxed)
    }
}

/// Input callback's end of the ring (or a test's). RT-safe.
#[derive(Debug)]
pub struct InputWriter {
    ring: rtrb::Producer<f32>,
    channels: usize,
    status: Arc<InputStatus>,
}

impl InputWriter {
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Queue interleaved samples, in whole frames. Frames that don't fit are
    /// dropped and counted.
    pub fn push(&mut self, samples: impl ExactSizeIterator<Item = f32>) {
        let frames = samples.len() / self.channels;
        let room = self.ring.slots() / self.channels;
        let written = frames.min(room);
        if written < frames {
            self.status
                .overflows
                .fetch_add((frames - written) as u64, Ordering::Relaxed);
        }
        if written > 0
            && let Ok(chunk) = self.ring.write_chunk_uninit(written * self.channels)
        {
            chunk.fill_from_iter(samples);
        }
    }

    /// Publish the device's latency (see [`InputStatus::latency_frames`]).
    pub fn set_latency_frames(&self, frames: usize) {
        self.status.latency.store(frames, Ordering::Relaxed);
    }
}

/// Audio thread's end of the ring. Create it off the audio thread and hand
/// it over with [`crate::engine::Engine::set_input`].
#[derive(Debug)]
pub struct InputFeed {
    ring: rtrb::Consumer<f32>,
    channels: usize,
    max_block: usize,
    target_fill: usize,
    /// More frames than this in the ring means the output stalled.
    high_water: usize,
    /// The last chunk pulled, one channel after the other, `max_block` apart.
    buffer: Box<[f32]>,
    /// Waiting for the ring to reach its target.
    filling: bool,
    /// The input has delivered audio: silence from now on is an underrun.
    started: bool,
    status: Arc<InputStatus>,
}

impl InputFeed {
    /// A feed of `channels` channels for an engine whose blocks are at most
    /// `max_block` frames. Pick a `target_fill` of at least one input buffer
    /// plus one output buffer: lower underruns, higher adds delay.
    pub fn new(channels: usize, max_block: usize, target_fill: usize) -> (Self, InputWriter) {
        let channels = channels.max(1);
        let target_fill = target_fill.max(1);
        let high_water = target_fill * 2 + max_block;
        let (producer, consumer) = rtrb::RingBuffer::new(high_water * 2 * channels);
        let status = Arc::new(InputStatus::new(channels));
        let feed = Self {
            ring: consumer,
            channels,
            max_block,
            target_fill,
            high_water,
            buffer: vec![0.0; channels * max_block].into_boxed_slice(),
            filling: true,
            started: false,
            status: status.clone(),
        };
        let writer = InputWriter {
            ring: producer,
            channels,
            status,
        };
        (feed, writer)
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn max_block(&self) -> usize {
        self.max_block
    }

    pub fn status(&self) -> Arc<InputStatus> {
        self.status.clone()
    }

    /// Take the next `frames` frames (at most `max_block`). Allocation-free
    /// and lock-free. What's returned is valid until the next pull.
    pub(crate) fn pull(&mut self, frames: usize) -> InputView {
        let frames = frames.min(self.max_block);
        let mut available = self.ring.slots() / self.channels;
        if available > self.high_water {
            let excess = available - self.target_fill;
            if let Ok(chunk) = self.ring.read_chunk(excess * self.channels) {
                chunk.commit_all();
            }
            self.status
                .overflows
                .fetch_add(excess as u64, Ordering::Relaxed);
            available = self.target_fill;
        }
        if available >= self.target_fill {
            self.filling = false;
        }
        let got = if self.filling {
            0
        } else {
            available.min(frames)
        };
        self.read_frames(got);
        if got < frames {
            if self.started {
                self.status
                    .underruns
                    .fetch_add((frames - got) as u64, Ordering::Relaxed);
            }
            self.filling = true;
        }
        self.started |= got > 0;
        for (channel, peak) in self
            .buffer
            .chunks_exact_mut(self.max_block.max(1))
            .zip(&self.status.peaks)
        {
            channel[got..frames].fill(0.0);
            let level = channel[..got].iter().fold(0.0f32, |m, s| m.max(s.abs()));
            if level > 0.0 {
                update_f32(peak, |p| p.max(level));
            }
        }
        self.status.fill.store(available - got, Ordering::Relaxed);
        InputView {
            data: self.buffer.as_ptr(),
            channels: self.channels,
            stride: self.max_block,
        }
    }

    /// Move `frames` frames from the ring to `buffer`.
    fn read_frames(&mut self, frames: usize) {
        let Ok(chunk) = self.ring.read_chunk(frames * self.channels) else {
            return;
        };
        let (first, second) = chunk.as_slices();
        let (mut channel, mut frame) = (0, 0);
        for &sample in first.iter().chain(second) {
            self.buffer[channel * self.max_block + frame] = sample;
            channel += 1;
            if channel == self.channels {
                channel = 0;
                frame += 1;
            }
        }
        chunk.commit_all();
    }
}

/// The input's audio for the block being processed, shared with every node
/// (workers read it in parallel). Nodes read it with
/// [`ProcessContext::device_input`](crate::graph::ProcessContext::device_input).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputView {
    data: *const f32,
    channels: usize,
    stride: usize,
}

// SAFETY: it's a read-only view. The engine only hands it out for a block,
// and the feed behind it isn't written to until every node has finished.
unsafe impl Send for InputView {}
unsafe impl Sync for InputView {}

impl InputView {
    /// No input.
    pub const NONE: Self = Self {
        data: std::ptr::null(),
        channels: 0,
        stride: 0,
    };

    /// `len` frames of `channel`, if there is such a channel.
    ///
    /// # Safety
    /// The block this view was pulled for must still be processing, with
    /// `len` no longer than that block.
    pub(crate) unsafe fn channel<'a>(self, channel: usize, len: usize) -> Option<&'a [f32]> {
        (channel < self.channels).then(|| unsafe {
            std::slice::from_raw_parts(self.data.add(channel * self.stride), len)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: usize = 8;
    const BLOCK: usize = 4;

    /// Two channels: `i` on the left and `-i` on the right, for frames `range`.
    fn push_frames(writer: &mut InputWriter, range: std::ops::Range<usize>) {
        let samples: Vec<f32> = range.flat_map(|i| [i as f32, -(i as f32)]).collect();
        writer.push(samples.into_iter());
    }

    fn left(feed: &mut InputFeed, frames: usize) -> Vec<f32> {
        let view = feed.pull(frames);
        // SAFETY: nothing pulls again while it's read.
        unsafe { view.channel(0, frames) }.unwrap().to_vec()
    }

    fn feed() -> (InputFeed, InputWriter) {
        InputFeed::new(2, BLOCK, TARGET)
    }

    #[test]
    fn stays_silent_until_the_target_fill_without_counting_an_underrun() {
        let (mut f, mut w) = feed();
        assert_eq!(left(&mut f, BLOCK), [0.0; BLOCK]);
        push_frames(&mut w, 1..TARGET); // one short
        assert_eq!(left(&mut f, BLOCK), [0.0; BLOCK]);
        push_frames(&mut w, TARGET..TARGET + 1);
        assert_eq!(left(&mut f, BLOCK), [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(f.status().underruns(), 0);
        assert_eq!(f.status().fill_frames(), TARGET - BLOCK);
    }

    #[test]
    fn deinterleaves_across_the_ring_wrapping_around() {
        let (mut f, mut w) = feed();
        push_frames(&mut w, 1..TARGET + 1);
        let mut next = 1;
        for _ in 0..200 {
            let view = f.pull(BLOCK);
            // SAFETY: nothing pulls again while they're read.
            let (l, r) = unsafe {
                (
                    view.channel(0, BLOCK).unwrap(),
                    view.channel(1, BLOCK).unwrap(),
                )
            };
            for i in 0..BLOCK {
                assert_eq!(l[i], (next + i) as f32);
                assert_eq!(r[i], -((next + i) as f32));
            }
            next += BLOCK;
            push_frames(&mut w, next + TARGET - BLOCK..next + TARGET);
        }
        assert_eq!(f.status().underruns(), 0);
        assert_eq!(f.status().overflows(), 0);
    }

    #[test]
    fn underrun_pads_with_silence_and_refills_to_the_target() {
        let (mut f, mut w) = feed();
        push_frames(&mut w, 1..TARGET + 1);
        assert_eq!(left(&mut f, BLOCK), [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(left(&mut f, 2), [5.0, 6.0]);
        // Two frames left for a block of four.
        assert_eq!(left(&mut f, BLOCK), [7.0, 8.0, 0.0, 0.0]);
        assert_eq!(f.status().underruns(), 2);

        // Not enough to play from yet: still silence, still counted.
        push_frames(&mut w, 9..12);
        assert_eq!(left(&mut f, BLOCK), [0.0; BLOCK]);
        assert_eq!(f.status().underruns(), 2 + BLOCK as u64);

        push_frames(&mut w, 12..9 + TARGET);
        assert_eq!(left(&mut f, BLOCK), [9.0, 10.0, 11.0, 12.0]);
        assert_eq!(f.status().underruns(), 2 + BLOCK as u64);
    }

    #[test]
    fn an_output_stall_drops_old_frames_back_to_the_target() {
        let (mut f, mut w) = feed();
        let high_water = TARGET * 2 + BLOCK;
        let capacity = high_water * 2;
        let pushed = capacity + 10;
        push_frames(&mut w, 1..pushed + 1);
        assert_eq!(f.status().overflows(), 10, "no room for these");

        // The newest frames the ring could hold are the target's worth up
        // to frame `capacity`.
        let first = (capacity - TARGET + 1) as f32;
        assert_eq!(left(&mut f, 2), [first, first + 1.0]);
        assert_eq!(
            f.status().overflows(),
            10 + (capacity - TARGET) as u64,
            "dropped to get back to the target"
        );
        assert_eq!(f.status().fill_frames(), TARGET - 2);
        assert_eq!(f.status().underruns(), 0);
    }

    #[test]
    fn jitter_below_the_high_water_mark_drops_nothing() {
        let (mut f, mut w) = feed();
        let high_water = TARGET * 2 + BLOCK;
        push_frames(&mut w, 1..high_water + 1);
        assert_eq!(left(&mut f, BLOCK), [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(f.status().overflows(), 0);
    }

    #[test]
    fn peaks_are_per_channel_and_reset_when_read() {
        let (mut f, mut w) = InputFeed::new(3, BLOCK, 2);
        let samples = [0.25, -0.5, 0.0, -0.75, 0.125, 0.0];
        w.push(samples.into_iter());
        f.pull(2);
        let status = f.status();
        assert_eq!(status.channels(), 3);
        assert_eq!(status.take_peak(0), 0.75);
        assert_eq!(status.take_peak(1), 0.5);
        assert_eq!(status.take_peak(2), 0.0);
        assert_eq!(status.take_peak(0), 0.0, "reset");
        assert_eq!(status.take_peak(9), 0.0, "no such channel");
    }

    #[test]
    fn a_partial_frame_is_not_queued() {
        let (mut f, mut w) = InputFeed::new(2, BLOCK, 1);
        w.push([0.5, -0.5, 0.25].into_iter());
        assert_eq!(left(&mut f, 1), [0.5]);
        assert_eq!(f.status().fill_frames(), 0);
    }

    #[test]
    fn a_view_has_only_its_own_channels() {
        let (mut f, _w) = feed();
        let view = f.pull(BLOCK);
        assert!(unsafe { view.channel(1, BLOCK) }.is_some());
        assert!(unsafe { view.channel(2, BLOCK) }.is_none());
        assert!(unsafe { InputView::NONE.channel(0, BLOCK) }.is_none());
    }
}
