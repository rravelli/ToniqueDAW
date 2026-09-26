//! Auditioning files (e.g. from a file browser), independent of the
//! transport and the graph.
//!
//! ```text
//!   Engine::preview_play/seek ──▶ feeder thread: PreviewSource::read,
//!                                 resample to the engine rate
//!                                          │ SPSC ring of stereo frames
//!                                          ▼  (a fresh ring per play/seek)
//!   AudioProcessor ── adds the ring's frames to its output, after the graph
//! ```
//!
//! Decoding and resampling happen on the feeder thread, so a source may
//! block and allocate. The RT side only pops from the ring; if it runs dry
//! it outputs silence. Every play or seek gets a new ring, so audio queued
//! for the old position is dropped instead of played.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::resample::{HermiteResampler, Resampler};

/// Audio to preview. Called on the feeder thread only.
pub trait PreviewSource: Send {
    fn sample_rate(&self) -> f64;
    /// Continue reading from `frame`.
    fn seek(&mut self, frame: usize);
    /// Fill up to `left.len()` frames (write the same data to both sides for
    /// mono). Returns the number of frames written; 0 means the end.
    fn read(&mut self, left: &mut [f32], right: &mut [f32]) -> usize;
}

pub(crate) type PreviewFrame = [f32; 2];
pub(crate) type PreviewStream = rtrb::Consumer<PreviewFrame>;

/// About 0.3 s at 48 kHz: plenty of slack for the feeder thread.
const RING_FRAMES: usize = 16384;
/// Source frames decoded per step.
const CHUNK: usize = 1024;

/// Written by the RT thread, read by the control side.
#[derive(Default)]
pub(crate) struct PreviewShared {
    /// Stream currently being played by the RT thread (0 = none).
    pub generation: AtomicU64,
    /// Frames of `generation` played so far.
    pub played: AtomicU64,
    /// Last generation that played to its end.
    pub finished: AtomicU64,
}

enum FeederMsg {
    /// Start a new source from `from`.
    Play { source: Box<dyn PreviewSource>, from: usize, ring: rtrb::Producer<PreviewFrame> },
    /// Continue the current source from `from`.
    Seek { from: usize, ring: rtrb::Producer<PreviewFrame> },
    Shutdown,
}

/// Control side of the preview player, owned by [`crate::engine::Engine`].
pub(crate) struct PreviewControl {
    shared: Arc<PreviewShared>,
    engine_rate: f64,
    feeder: Option<(Sender<FeederMsg>, JoinHandle<()>)>,
    /// Last generation handed out (play, seek or stop).
    generation: u64,
    /// The generation that should be playing; `None` once stopped.
    playing: Option<u64>,
    /// Source position where `generation` starts, and source frames per
    /// engine frame.
    start: usize,
    ratio: f64,
}

impl PreviewControl {
    pub fn new(engine_rate: f64) -> Self {
        Self { shared: Arc::default(), engine_rate, feeder: None, generation: 0, playing: None, start: 0, ratio: 1.0 }
    }

    pub fn shared(&self) -> Arc<PreviewShared> {
        self.shared.clone()
    }

    fn feeder(&mut self) -> &Sender<FeederMsg> {
        let engine_rate = self.engine_rate;
        &self
            .feeder
            .get_or_insert_with(|| {
                let (tx, rx) = channel();
                let handle = std::thread::Builder::new()
                    .name("tonique-preview".into())
                    .spawn(move || run_feeder(rx, engine_rate))
                    .expect("spawn preview thread");
                (tx, handle)
            })
            .0
    }

    /// Returns the stream to hand to the RT thread and its generation.
    pub fn play(&mut self, source: Box<dyn PreviewSource>, from: usize) -> (PreviewStream, u64) {
        self.ratio = source.sample_rate() / self.engine_rate;
        let (ring, stream) = rtrb::RingBuffer::new(RING_FRAMES);
        let _ = self.feeder().send(FeederMsg::Play { source, from, ring });
        self.restart(stream, from)
    }

    /// `None` if nothing was ever played (no source to seek in).
    pub fn seek(&mut self, from: usize) -> Option<(PreviewStream, u64)> {
        self.feeder.as_ref()?;
        let (ring, stream) = rtrb::RingBuffer::new(RING_FRAMES);
        let _ = self.feeder().send(FeederMsg::Seek { from, ring });
        Some(self.restart(stream, from))
    }

    fn restart(&mut self, stream: PreviewStream, from: usize) -> (PreviewStream, u64) {
        self.generation += 1;
        self.playing = Some(self.generation);
        self.start = from;
        (stream, self.generation)
    }

    /// Returns the generation that tells the RT thread to drop its stream.
    pub fn stop(&mut self) -> u64 {
        self.generation += 1;
        self.playing = None;
        self.generation
    }

    /// Whether audio is playing, or about to: a play/seek counts from the
    /// moment it's requested, until its audio has all been played.
    pub fn is_playing(&self) -> bool {
        self.playing.is_some_and(|g| self.shared.finished.load(Ordering::Relaxed) != g)
    }

    /// Position in source frames, while playing.
    pub fn position(&self) -> Option<usize> {
        let g = self.playing.filter(|_| self.is_playing())?;
        let played = if self.shared.generation.load(Ordering::Relaxed) == g {
            self.shared.played.load(Ordering::Relaxed)
        } else {
            0 // not picked up by the RT thread yet
        };
        Some(self.start + (played as f64 * self.ratio) as usize)
    }
}

impl Drop for PreviewControl {
    fn drop(&mut self) {
        if let Some((tx, handle)) = self.feeder.take() {
            let _ = tx.send(FeederMsg::Shutdown);
            let _ = handle.join();
        }
    }
}

/// Feeder thread: keeps the current ring topped up from the current source.
fn run_feeder(rx: Receiver<FeederMsg>, engine_rate: f64) {
    let mut source: Option<Box<dyn PreviewSource>> = None;
    let mut ring: Option<rtrb::Producer<PreviewFrame>> = None;
    let mut resamplers = [HermiteResampler::new(1.0), HermiteResampler::new(1.0)];
    let (mut input, mut output) = ([vec![0.0; CHUNK], vec![0.0; CHUNK]], [Vec::new(), Vec::new()]);

    loop {
        // Block while idle; otherwise just check for news between chunks.
        let idle = ring.as_ref().is_none_or(|r| r.is_abandoned());
        let msg = if idle { rx.recv().map_err(|_| RecvTimeoutError::Disconnected) } else { rx.recv_timeout(Duration::ZERO) };
        match msg {
            Ok(FeederMsg::Play { source: s, from, ring: r }) => {
                source = Some(s);
                ring = Some(r);
                resamplers = restart(&mut source, from, engine_rate);
                continue;
            }
            Ok(FeederMsg::Seek { from, ring: r }) => {
                ring = Some(r);
                resamplers = restart(&mut source, from, engine_rate);
                continue;
            }
            Ok(FeederMsg::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let (Some(src), Some(r)) = (source.as_mut(), ring.as_mut()) else { continue };
        // Room for a whole resampled chunk (upsampling makes it longer).
        let ratio = src.sample_rate() / engine_rate;
        let out_max = (CHUNK as f64 / ratio).ceil() as usize + 4;
        if r.slots() < out_max {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        let [l, rt] = &mut input;
        let n = src.read(l, rt);
        if n == 0 {
            ring = None; // abandons the ring: the RT side finishes once it's drained
            continue;
        }
        let mut produced = n;
        if ratio != 1.0 {
            for ((resampler, inp), out) in resamplers.iter_mut().zip(&input).zip(&mut output) {
                out.resize(out_max, 0.0);
                produced = resampler.process(&inp[..n], out).1;
            }
        } else {
            for (inp, out) in input.iter().zip(&mut output) {
                out.clear();
                out.extend_from_slice(&inp[..n]);
            }
        }
        for (&left, &right) in output[0][..produced].iter().zip(&output[1][..produced]) {
            let _ = r.push([left, right]);
        }
    }
}

fn restart(source: &mut Option<Box<dyn PreviewSource>>, from: usize, engine_rate: f64) -> [HermiteResampler; 2] {
    let ratio = source.as_mut().map_or(1.0, |s| {
        s.seek(from);
        s.sample_rate() / engine_rate
    });
    [HermiteResampler::new(ratio), HermiteResampler::new(ratio)]
}
