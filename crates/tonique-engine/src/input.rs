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
//!
//! An input at another rate, or on another clock, goes through a resampler
//! on its way out of the ring. Its ratio is steered to keep the ring where
//! it settled when the input started, which makes up for the clocks'
//! difference without ever dropping a frame, or moving what's being recorded.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::meter::update_f32;
use crate::resample::{DriftCorrector, SincResampler};

/// The ring's target for an input and an output delivering and asking for
/// this many frames per callback, in input frames. Both can be a callback
/// late, and a resampler at times takes a frame more than its share: with no
/// margin, that runs the ring dry, and silence until it's full again is far
/// more than the delay this adds.
pub fn target_fill(input_frames: usize, output_frames: usize) -> usize {
    input_frames + output_frames + input_frames.max(output_frames) / 2
}

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
    mean_fill: AtomicUsize,
    /// Engine frames per input frame.
    engine_per_input: f64,
    /// Input frames of delay in the resampler.
    filter_delay: usize,
    /// Input frames of the input's last delivery.
    input_block: AtomicUsize,
    /// The input's callbacks keep their own time, not the output's.
    free_running: bool,
}

impl InputStatus {
    fn new(
        channels: usize,
        engine_per_input: f64,
        filter_delay: usize,
        free_running: bool,
    ) -> Self {
        Self {
            peaks: (0..channels).map(|_| AtomicU32::new(0)).collect(),
            underruns: AtomicU64::new(0),
            overflows: AtomicU64::new(0),
            latency: AtomicUsize::new(0),
            fill: AtomicUsize::new(0),
            mean_fill: AtomicUsize::new(0),
            engine_per_input,
            filter_delay,
            input_block: AtomicUsize::new(0),
            free_running,
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

    /// Input frames from the device capturing audio to its callback
    /// delivering it.
    pub fn latency_frames(&self) -> usize {
        self.latency.load(Ordering::Relaxed)
    }

    /// Input frames left in the ring after the last pull.
    pub fn fill_frames(&self) -> usize {
        self.fill.load(Ordering::Relaxed)
    }

    /// Input frames the ring delays the input by: how many it holds as an
    /// output callback starts, on average, as that moves with both callbacks'
    /// timing. When resampling, the fill it's steered to keep.
    pub fn mean_fill_frames(&self) -> usize {
        self.mean_fill.load(Ordering::Relaxed)
    }

    /// Engine frames from the device capturing audio to the engine reading
    /// it: the device, the ring and the resampler. On a clock of its own the
    /// input also delivered its last block some time ago when the output
    /// reads the ring, half a block on average, while on the same clock it
    /// does so right before.
    pub fn delay_frames(&self) -> usize {
        let since_delivery = if self.free_running {
            self.input_block.load(Ordering::Relaxed) / 2
        } else {
            0
        };
        let input_frames =
            self.latency_frames() + self.mean_fill_frames() + self.filter_delay + since_delivery;
        (input_frames as f64 * self.engine_per_input).round() as usize
    }
}

/// Share of each new reading in [`InputStatus::mean_fill_frames`]: settles
/// in about 50 callbacks.
const MEAN_FILL_WEIGHT: f64 = 0.02;

/// Callbacks to wait for the ring's fill to settle, before steering to keep
/// it there.
const SETTLING_CALLBACKS: u32 = 200;

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
        self.status.input_block.store(frames, Ordering::Relaxed);
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
    mean_fill: f64,
    /// The ring has just reached its target: the mean starts over from it.
    fill_restarted: bool,
    /// `None` when the input runs on the engine's clock.
    resampling: Option<Box<Resampling>>,
    status: Arc<InputStatus>,
}

#[derive(Debug)]
struct Resampling {
    resampler: SincResampler,
    corrector: DriftCorrector,
    engine_rate: f64,
    /// Callbacks left before steering starts.
    settling: u32,
    /// The fill it's steered to keep.
    held_fill: f64,
}

impl InputFeed {
    /// A feed of `channels` channels for an engine whose blocks are at most
    /// `max_block` frames, from an input on the engine's clock. See
    /// [`target_fill`]: lower underruns, higher adds delay.
    pub fn new(channels: usize, max_block: usize, target_fill: usize) -> (Self, InputWriter) {
        Self::build(channels, max_block, target_fill, 1.0, None)
    }

    /// A feed from an input at another rate, or on another clock even at the
    /// same rate. `target_fill` is in input frames.
    pub fn resampled(
        channels: usize,
        max_block: usize,
        target_fill: usize,
        input_rate: f64,
        engine_rate: f64,
    ) -> (Self, InputWriter) {
        let channels = channels.max(1);
        let ratio = input_rate / engine_rate;
        let resampling = Resampling {
            resampler: SincResampler::new(channels, ratio),
            corrector: DriftCorrector::new(ratio, target_fill.max(1), input_rate),
            engine_rate,
            settling: SETTLING_CALLBACKS,
            held_fill: target_fill.max(1) as f64,
        };
        Self::build(
            channels,
            max_block,
            target_fill,
            ratio,
            Some(Box::new(resampling)),
        )
    }

    fn build(
        channels: usize,
        max_block: usize,
        target_fill: usize,
        ratio: f64,
        resampling: Option<Box<Resampling>>,
    ) -> (Self, InputWriter) {
        let channels = channels.max(1);
        let target_fill = target_fill.max(1);
        let block_in_input_frames = (max_block as f64 * ratio).ceil() as usize;
        let high_water = target_fill * 2 + block_in_input_frames;
        let (producer, consumer) = rtrb::RingBuffer::new(high_water * 2 * channels);
        let filter_delay = resampling
            .as_ref()
            .map_or(0, |r| r.resampler.delay_frames());
        let status = Arc::new(InputStatus::new(
            channels,
            1.0 / ratio,
            filter_delay,
            resampling.is_some(),
        ));
        status.mean_fill.store(target_fill, Ordering::Relaxed);
        let feed = Self {
            ring: consumer,
            channels,
            max_block,
            target_fill,
            high_water,
            buffer: vec![0.0; channels * max_block].into_boxed_slice(),
            filling: true,
            started: false,
            mean_fill: target_fill as f64,
            fill_restarted: false,
            resampling,
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

    /// Note how full the ring is as an output callback of `frames` frames
    /// starts, and steer the resampler by it.
    pub(crate) fn begin_callback(&mut self, frames: usize) {
        if self.filling {
            return;
        }
        let fill = (self.ring.slots() / self.channels) as f64;
        if std::mem::take(&mut self.fill_restarted) {
            self.mean_fill = fill;
            if let Some(r) = &mut self.resampling {
                r.settling = SETTLING_CALLBACKS;
            }
        }
        self.mean_fill += (fill - self.mean_fill) * MEAN_FILL_WEIGHT;
        let mut delay = self.mean_fill;
        if let Some(r) = &mut self.resampling {
            if r.settling > 0 {
                r.settling -= 1;
                r.held_fill = self.mean_fill;
                r.corrector.set_target_fill(r.held_fill);
            }
            let seconds = frames as f64 / r.engine_rate;
            r.resampler
                .set_ratio(r.corrector.update(self.mean_fill, seconds));
            // Where the fill is going to stay, whatever it's doing right now.
            delay = r.held_fill;
        }
        self.status
            .mean_fill
            .store(delay.round() as usize, Ordering::Relaxed);
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
        if self.filling && available >= self.target_fill {
            self.filling = false;
            self.fill_restarted = true;
        }
        let (consumed, got) = if self.filling {
            (0, 0)
        } else {
            self.read_frames(available, frames)
        };
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
        self.status
            .fill
            .store(available - consumed, Ordering::Relaxed);
        InputView {
            data: self.buffer.as_ptr(),
            channels: self.channels,
            stride: self.max_block,
        }
    }

    /// Make up to `frames` frames in `buffer` out of the `available` frames
    /// in the ring. Returns how many were consumed and made.
    fn read_frames(&mut self, available: usize, frames: usize) -> (usize, usize) {
        let Some(r) = &mut self.resampling else {
            let n = available.min(frames);
            self.copy_frames(n);
            return (n, n);
        };
        let Ok(chunk) = self.ring.read_chunk(available * self.channels) else {
            return (0, 0);
        };
        let (first, second) = chunk.as_slices();
        let stride = self.max_block;
        let (mut consumed, mut made) =
            r.resampler
                .process(first, &mut self.buffer, stride, 0, frames);
        if made < frames {
            let (c, m) = r
                .resampler
                .process(second, &mut self.buffer, stride, made, frames - made);
            consumed += c;
            made += m;
        }
        chunk.commit(consumed * self.channels);
        (consumed, made)
    }

    /// Move `frames` frames from the ring to `buffer`.
    fn copy_frames(&mut self, frames: usize) {
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
    fn the_mean_fill_starts_at_the_target_and_follows_callbacks() {
        let (mut f, mut w) = feed();
        let status = f.status();
        assert_eq!(status.mean_fill_frames(), TARGET);
        // Still filling: not representative.
        f.begin_callback(BLOCK);
        assert_eq!(status.mean_fill_frames(), TARGET);

        push_frames(&mut w, 1..TARGET + 1);
        f.pull(BLOCK);
        // Holding 12 frames as callbacks start, from now on.
        push_frames(&mut w, 0..BLOCK * 2);
        for _ in 0..1000 {
            f.begin_callback(BLOCK);
            f.pull(BLOCK);
            push_frames(&mut w, 0..BLOCK);
        }
        assert_eq!(status.mean_fill_frames(), 12);
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

    /// Two devices with the blocks `(input, output)`, each at its rate:
    /// `seconds` of audio through a feed, every `late_every`th input delivery
    /// one block late if given. Returns the feed and the fills seen as
    /// callbacks started over the last half of it.
    fn run_clocks_with(
        (in_block, out_block): (usize, usize),
        (input_rate, engine_rate, real_input_rate): (f64, f64, f64),
        seconds: f64,
        target: usize,
        late_every: Option<usize>,
    ) -> (InputFeed, Vec<usize>) {
        let (mut f, mut w) = InputFeed::resampled(1, out_block, target, input_rate, engine_rate);
        let callbacks = (seconds * engine_rate / out_block as f64) as usize;
        let mut delivered = 0usize;
        let mut fills = Vec::new();
        for callback in 0..callbacks {
            let now = (callback * out_block) as f64 / engine_rate;
            loop {
                let next = delivered + 1;
                let late = late_every.is_some_and(|n| next.is_multiple_of(n));
                let due = ((next + late as usize) * in_block) as f64 / real_input_rate;
                if due > now {
                    break;
                }
                w.push(std::iter::repeat_n(0.5, in_block));
                delivered = next;
            }
            f.begin_callback(out_block);
            if callback >= callbacks / 2 {
                fills.push(f.ring.slots());
            }
            f.pull(out_block);
        }
        (f, fills)
    }

    /// In block sizes sharing no factor, so deliveries fall at every point
    /// of the output's callbacks, as those of a real device do.
    fn run_clocks(
        input_rate: f64,
        engine_rate: f64,
        real_input_rate: f64,
        seconds: f64,
    ) -> (InputFeed, Vec<usize>) {
        const BLOCKS: (usize, usize) = (479, 256);
        let output = (BLOCKS.1 as f64 * input_rate / engine_rate).ceil() as usize;
        run_clocks_with(
            BLOCKS,
            (input_rate, engine_rate, real_input_rate),
            seconds,
            target_fill(BLOCKS.0, output),
            None,
        )
    }

    /// As seen on a real stream through a sound server, whose callbacks are
    /// the same size and at times late.
    #[test]
    fn late_deliveries_do_not_run_the_ring_dry() {
        for ppm in [-100.0, 0.0, 100.0] {
            let real = 48000.0 * (1.0 + ppm * 1e-6);
            let target = target_fill(512, 512);
            let rates = (48000.0, 48000.0, real);
            let (f, _) = run_clocks_with((512, 512), rates, 120.0, target, Some(97));
            assert_eq!(f.status.underruns(), 0, "{ppm} ppm");
        }
    }

    /// Why [`target_fill`] has a margin.
    #[test]
    fn no_margin_runs_dry_on_a_late_delivery() {
        let rates = (48000.0, 48000.0, 48000.0);
        let (f, _) = run_clocks_with((512, 512), rates, 120.0, 1024, Some(97));
        assert!(f.status.underruns() > 0);
    }

    /// The ring stays where it settled, within a frame or two.
    fn assert_no_dropouts_and_steady(f: &InputFeed, fills: &[usize]) {
        assert_eq!(f.status.underruns(), 0);
        assert_eq!(f.status.overflows(), 0);
        let mean = |part: &[usize]| part.iter().sum::<usize>() as f64 / part.len() as f64;
        let (first, second) = fills.split_at(fills.len() / 2);
        let (first, second) = (mean(first), mean(second));
        assert!((first - second).abs() < 2.0, "fill {first}, then {second}");
        assert!(
            second >= f.target_fill as f64 * 0.75,
            "fill {second}, close to running dry"
        );
    }

    #[test]
    fn rates_that_differ_keep_the_ring_at_its_target() {
        for (from, to) in [(44100.0, 48000.0), (48000.0, 44100.0), (96000.0, 48000.0)] {
            let (f, fills) = run_clocks(from, to, from, 60.0);
            assert_no_dropouts_and_steady(&f, &fills);
        }
    }

    #[test]
    fn clocks_apart_at_the_same_rate_never_drop_a_frame() {
        for ppm in [-200.0, -30.0, 30.0, 200.0] {
            let real = 48000.0 * (1.0 + ppm * 1e-6);
            let (f, fills) = run_clocks(48000.0, 48000.0, real, 300.0);
            assert_no_dropouts_and_steady(&f, &fills);
        }
    }

    #[test]
    fn delay_is_in_engine_frames_and_counts_the_filter() {
        let (f, w) = InputFeed::resampled(1, 256, 1000, 96000.0, 48000.0);
        w.set_latency_frames(200);
        let filter = f.resampling.as_ref().unwrap().resampler.delay_frames();
        assert_eq!(
            f.status.delay_frames(),
            (200usize + 1000 + filter).div_ceil(2)
        );

        let (f, w) = InputFeed::new(1, 256, 1000);
        w.set_latency_frames(200);
        assert_eq!(f.status.delay_frames(), 1200);
    }

    #[test]
    fn a_resampled_underrun_pads_with_silence_and_counts() {
        let (mut f, mut w) = InputFeed::resampled(1, 64, 200, 48000.0, 48000.0);
        w.push(std::iter::repeat_n(0.5, 200));
        f.pull(64);
        f.pull(64);
        f.pull(64);
        assert_eq!(f.status.underruns(), 0);
        let view = f.pull(64);
        let samples = unsafe { view.channel(0, 64) }.unwrap();
        assert_eq!(samples[63], 0.0);
        assert!(f.status.underruns() > 0);
    }
}
