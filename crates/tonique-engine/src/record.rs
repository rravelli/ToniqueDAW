//! Recording input to WAV files, without the audio thread touching a disk.
//!
//! ```text
//!   AudioProcessor, per chunk ── Capture ─┬─ frames ring ─▶ take writer thread:
//!                                         └─ events ring ─▶ Splitter ─▶ one WAV
//!                                                           per pass and track
//! ```
//!
//! The audio thread only pushes input frames and a log of what the transport
//! did. The frame read while rendering frame `g` was played to at `g - L`,
//! `L` being the round trip through the output and input (see
//! [`crate::engine::Engine::record_start`]). So frame `f` belongs where the
//! transport was at frame `f - L`, which the writer thread works out from the
//! log. A jump there, such as the loop going round, starts a new pass.
//!
//! Capture goes on for `L` frames after punching out, as that's when what was
//! played up to then has come back.

use std::collections::VecDeque;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use crate::input::{InputRoute, InputStatus, InputView};
use crate::nodes::Click;
use crate::sample::WavWriter;
use crate::time::{BeatPos, SamplePos, TempoMap};

/// What the frames ring can hold, for the writer thread stalling on a disk.
const RING_SECONDS: f64 = 4.0;
const EVENT_CAPACITY: usize = 4096;
/// Frames written at a time.
const WRITE_CHUNK: usize = 4096;
const IDLE: Duration = Duration::from_millis(5);
/// How much a crash can lose of a take.
const FLUSH_EVERY: Duration = Duration::from_secs(1);
/// Passes shorter than this are dropped: a loop going round just after
/// punching in, say.
const MIN_PASS_SECONDS: f64 = 0.1;

/// What the transport did from `frame` on, until the next segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Segment {
    pub frame: u64,
    pub pos: SamplePos,
    pub playing: bool,
    /// Only what's inside gets recorded.
    pub loop_range: Option<(SamplePos, SamplePos)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum CaptureEvent {
    Segment(Segment),
    /// Frames that didn't fit in the ring. They stay on the timeline as
    /// silence.
    Gap {
        frame: u64,
        frames: u64,
    },
}

/// Frames to write, and where they go on the timeline.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Span {
    pub frames: Range<u64>,
    pub start: SamplePos,
    /// Not the continuation of the previous span.
    pub new_pass: bool,
}

/// Works out where captured frames belong, from the transport's log. Free of
/// any I/O. Frames are given in order, and the segments of a frame `f` must
/// be known by the time it's given: the audio thread logs them first.
#[derive(Debug)]
pub(crate) struct Splitter {
    latency: u64,
    segments: VecDeque<Segment>,
    /// Frames dealt with so far.
    done: u64,
    /// Where the last span ended, in frames and on the timeline.
    last_end: Option<(u64, SamplePos)>,
}

impl Splitter {
    pub fn new(latency: u64) -> Self {
        Self {
            latency,
            segments: VecDeque::new(),
            done: 0,
            last_end: None,
        }
    }

    pub fn push(&mut self, segment: Segment) {
        self.segments.push_back(segment);
    }

    /// Go through the frames up to `end`, calling `emit` for those to write.
    pub fn advance(&mut self, end: u64, mut emit: impl FnMut(Span)) {
        while self.done < end {
            let Some(segment) = self.segments.front().copied() else {
                self.done = end; // nothing was played yet when these came back
                return;
            };
            let begin = segment.frame + self.latency;
            if self.done < begin {
                self.done = begin.min(end);
                continue;
            }
            let until = match self.segments.get(1).copied() {
                Some(next) if next.frame + self.latency <= end => {
                    self.segments.pop_front();
                    next.frame + self.latency
                }
                _ => end,
            };
            if let Some(span) = self.span(&segment, begin, until) {
                emit(span);
            }
            self.done = until;
        }
    }

    /// What to write of a segment's frames up to `until`, not yet done.
    fn span(&mut self, segment: &Segment, begin: u64, until: u64) -> Option<Span> {
        if !segment.playing || until <= self.done {
            return None;
        }
        let pos_at = |frame: u64| segment.pos + (frame - begin) as SamplePos;
        let (mut from, mut to) = (pos_at(self.done), pos_at(until));
        if let Some((start, end)) = segment.loop_range {
            from = from.max(start);
            to = to.min(end);
        }
        if from >= to {
            return None;
        }
        let first = begin + (from - segment.pos) as u64;
        let last = begin + (to - segment.pos) as u64;
        let new_pass = self.last_end != Some((first, from));
        self.last_end = Some((last, to));
        Some(Span {
            frames: first..last,
            start: from,
            new_pass,
        })
    }
}

/// Clicks the bars before recording starts, with the transport stopped.
/// Build it off the audio thread, for [`RecordOptions::count_in`].
#[derive(Debug)]
pub struct CountIn {
    /// When each click starts, and whether it's the first of a bar.
    clicks: Box<[(u64, bool)]>,
    next_click: usize,
    length: u64,
    frame: u64,
    gain: f32,
    dt: f32,
    click: Click,
}

impl CountIn {
    /// `bars` bars at the tempo and time signature at `start`. `None` for no
    /// bars.
    pub fn new(
        tempo: &TempoMap,
        start: BeatPos,
        bars: u32,
        sample_rate: f64,
        gain: f32,
    ) -> Option<Self> {
        let signature = tempo.time_signature;
        let frames_per_beat = 60.0 / tempo.bpm_at(start) * sample_rate;
        let frames_per_click = signature.beat_length() * frames_per_beat;
        let clicks: Box<[(u64, bool)]> = (0..bars * signature.numerator)
            .map(|i| {
                let at = (i as f64 * frames_per_click).round() as u64;
                (at, i % signature.numerator == 0)
            })
            .collect();
        (!clicks.is_empty()).then(|| Self {
            length: (bars as f64 * signature.beats_per_bar() * frames_per_beat).round() as u64,
            clicks,
            next_click: 0,
            frame: 0,
            gain,
            dt: 1.0 / sample_rate as f32,
            click: Click::default(),
        })
    }

    /// Frames until the end.
    pub(crate) fn remaining(&self) -> u64 {
        self.length.saturating_sub(self.frame)
    }

    /// Add the next frames (`out` holds as many) to the first two channels.
    pub(crate) fn mix(&mut self, out: &mut [f32], channels: usize) {
        for frame in out.chunks_exact_mut(channels.max(1)) {
            if let Some(&(at, accent)) = self.clicks.get(self.next_click)
                && at == self.frame
            {
                self.click.start(accent);
                self.next_click += 1;
            }
            let s = self.click.next(self.dt) * self.gain;
            for sample in frame.iter_mut().take(2) {
                *sample += s;
            }
            self.frame += 1;
        }
    }
}

#[derive(Debug, Default)]
struct CaptureShared {
    /// Everything has been pushed.
    finished: AtomicBool,
    /// Frames that didn't fit in the ring.
    dropped: AtomicU64,
}

/// Transport state last logged.
#[derive(Clone, Copy, Debug)]
struct Logged {
    /// Where it would be now without a jump.
    pos: SamplePos,
    playing: bool,
    loop_range: Option<(SamplePos, SamplePos)>,
}

/// Audio thread's end of a take. Allocation-free and lock-free.
#[derive(Debug)]
pub struct Capture {
    frames: rtrb::Producer<f32>,
    events: rtrb::Producer<CaptureEvent>,
    /// Input channels captured, in the order they're pushed.
    sources: Box<[usize]>,
    latency: u64,
    frame: u64,
    logged: Option<Logged>,
    /// Frames dropped since the last gap was logged: (first, how many).
    gap: Option<(u64, u64)>,
    /// When to end, once told to stop.
    stop_at: Option<u64>,
    shared: Arc<CaptureShared>,
}

impl Capture {
    /// Take the input's frames for a chunk of `n` frames at `pos`. Channels
    /// the input lacks are captured as silence.
    pub(crate) fn record(
        &mut self,
        input: InputView,
        n: usize,
        pos: SamplePos,
        playing: bool,
        loop_range: Option<(SamplePos, SamplePos)>,
    ) {
        if self.is_finished() {
            return;
        }
        if self.stop_at.is_some_and(|end| self.frame >= end) {
            self.finish();
            return;
        }
        let playing = playing && self.stop_at.is_none();
        // A gap and a segment: without room, try again on the next chunk.
        if self.events.slots() < 2 {
            self.drop_frames(n);
        } else {
            self.log(pos, playing, loop_range);
            self.push_frames(input, n);
        }
        self.frame += n as u64;
    }

    /// Log the transport state if it isn't what follows from the last one.
    fn log(&mut self, pos: SamplePos, playing: bool, loop_range: Option<(SamplePos, SamplePos)>) {
        let changed = self.logged.is_none_or(|last| {
            last.playing != playing || last.loop_range != loop_range || (playing && last.pos != pos)
        });
        if changed {
            let _ = self.events.push(CaptureEvent::Segment(Segment {
                frame: self.frame,
                pos,
                playing,
                loop_range,
            }));
            self.logged = Some(Logged {
                pos,
                playing,
                loop_range,
            });
        }
    }

    fn push_frames(&mut self, input: InputView, n: usize) {
        let channels = self.sources.len();
        if self.frames.slots() < n * channels {
            self.drop_frames(n);
            return;
        }
        if let Some((frame, frames)) = self.gap.take() {
            let _ = self.events.push(CaptureEvent::Gap { frame, frames });
        }
        if let Ok(chunk) = self.frames.write_chunk_uninit(n * channels) {
            let sources = &self.sources;
            chunk.fill_from_iter((0..n).flat_map(|f| {
                sources.iter().map(move |&c| {
                    // SAFETY: called while the chunk's block is processed.
                    unsafe { input.channel(c, n) }.map_or(0.0, |s| s[f])
                })
            }));
        }
        if let Some(logged) = &mut self.logged
            && logged.playing
        {
            logged.pos += n as SamplePos;
        }
    }

    fn drop_frames(&mut self, n: usize) {
        let (_, frames) = self.gap.get_or_insert((self.frame, 0));
        *frames += n as u64;
        self.shared.dropped.fetch_add(n as u64, Ordering::Relaxed);
    }

    /// Punch out: keep capturing what has yet to come back, then end.
    pub(crate) fn stop(&mut self) {
        if self.stop_at.is_none() {
            self.stop_at = Some(self.frame + self.latency);
        }
    }

    fn finish(&mut self) {
        if let Some((frame, frames)) = self.gap.take() {
            let _ = self.events.push(CaptureEvent::Gap { frame, frames });
        }
        self.shared.finished.store(true, Ordering::Release);
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.shared.finished.load(Ordering::Relaxed)
    }
}

/// A track to record into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordTrack {
    /// Comes back in [`TakeFile::track`].
    pub track: u64,
    /// Mono makes 1-channel files, stereo 2-channel files.
    pub route: InputRoute,
    /// Files are named "`name` take N.wav".
    pub name: String,
}

/// What [`crate::engine::Engine::record_start`] records.
#[derive(Debug)]
pub struct RecordOptions {
    /// Where the files go. Created if needed.
    pub directory: PathBuf,
    pub tracks: Vec<RecordTrack>,
    /// Added to the latency reported by the devices, for when they're wrong.
    pub latency_offset_frames: i64,
    /// Used only if the transport is stopped, which it then starts.
    pub count_in: Option<CountIn>,
}

/// A track's file for a pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TakeFile {
    pub track: u64,
    pub path: PathBuf,
}

/// An unbroken stretch of the timeline that was recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TakePass {
    pub start: SamplePos,
    pub frames: u64,
    pub files: Vec<TakeFile>,
}

/// How a take went.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TakeReport {
    /// In the order they were recorded.
    pub passes: Vec<TakePass>,
    /// Frames lost on the way, silent in the files.
    pub dropouts: u64,
    /// Frames the take was moved back by.
    pub latency: u64,
    /// What went wrong, if the take ended early. The passes are what was
    /// written until then.
    pub error: Option<String>,
}

/// Writer thread's end of a take.
pub(crate) struct TakeWriter {
    frames: rtrb::Consumer<f32>,
    events: rtrb::Consumer<CaptureEvent>,
    shared: Arc<CaptureShared>,
    channels: usize,
    splitter: Splitter,
    gaps: VecDeque<(u64, u64)>,
    cursor: u64,
    files: PassFiles,
    input: Arc<InputStatus>,
    dropouts_before: u64,
}

/// Make the two ends of a take, for frames of `sources` of the input.
pub(crate) fn new_take(
    options: RecordOptions,
    sources: Box<[usize]>,
    ring_frames: usize,
    latency: u64,
    sample_rate: f64,
    input: Arc<InputStatus>,
) -> (Capture, TakeWriter) {
    let channels = sources.len();
    let (frames_tx, frames_rx) = rtrb::RingBuffer::new(ring_frames * channels);
    let (events_tx, events_rx) = rtrb::RingBuffer::new(EVENT_CAPACITY);
    let shared = Arc::new(CaptureShared::default());
    let tracks = options
        .tracks
        .iter()
        .map(|t| TrackFiles::new(t, &sources))
        .collect();
    let capture = Capture {
        frames: frames_tx,
        events: events_tx,
        sources,
        latency,
        frame: 0,
        logged: None,
        gap: None,
        stop_at: None,
        shared: shared.clone(),
    };
    let writer = TakeWriter {
        frames: frames_rx,
        events: events_rx,
        shared,
        channels,
        splitter: Splitter::new(latency),
        gaps: VecDeque::new(),
        cursor: 0,
        files: PassFiles {
            directory: options.directory,
            tracks,
            sample_rate,
            min_frames: (MIN_PASS_SECONDS * sample_rate) as u64,
            open: None,
            passes: Vec::new(),
            error: None,
            scratch: Vec::new(),
            latency,
        },
        dropouts_before: input.underruns() + input.overflows(),
        input,
    };
    (capture, writer)
}

pub(crate) fn ring_frames(sample_rate: f64) -> usize {
    (RING_SECONDS * sample_rate) as usize
}

/// The input channels `tracks` need, each once.
pub(crate) fn sources_of(tracks: &[RecordTrack]) -> Box<[usize]> {
    let mut sources: Vec<usize> = tracks
        .iter()
        .flat_map(|t| match t.route {
            InputRoute::Mono(c) => vec![c],
            InputRoute::Stereo(c) => vec![c, c + 1],
        })
        .collect();
    sources.sort_unstable();
    sources.dedup();
    sources.into()
}

impl TakeWriter {
    /// Write until the capture ends, and report on it.
    pub(crate) fn run(mut self, report: Sender<TakeReport>) {
        let mut scratch = vec![0.0; WRITE_CHUNK * self.channels];
        let mut last_flush = Instant::now();
        loop {
            // In this order: a finished capture has pushed everything, and
            // events come before the frames they're about.
            let finished =
                self.shared.finished.load(Ordering::Acquire) || self.frames.is_abandoned();
            let available = self.frames.slots() / self.channels;
            while let Ok(event) = self.events.pop() {
                match event {
                    CaptureEvent::Segment(s) => self.splitter.push(s),
                    CaptureEvent::Gap { frame, frames } => self.gaps.push_back((frame, frames)),
                }
            }
            let wrote = self.write_available(available, &mut scratch);
            if finished && self.gaps.is_empty() && self.frames.slots() == 0 {
                break;
            }
            if last_flush.elapsed() >= FLUSH_EVERY {
                self.files.flush();
                last_flush = Instant::now();
            }
            if !wrote {
                std::thread::sleep(IDLE);
            }
        }
        let _ = report.send(self.finish());
    }

    fn write_available(&mut self, mut available: usize, scratch: &mut [f32]) -> bool {
        let mut wrote = false;
        loop {
            let gap = self.gaps.front().copied();
            let n = match gap {
                Some((frame, frames)) if frame == self.cursor => {
                    let n = frames.min(WRITE_CHUNK as u64);
                    self.gaps.pop_front();
                    if frames > n {
                        self.gaps.push_front((frame + n, frames - n));
                    }
                    scratch[..n as usize * self.channels].fill(0.0);
                    n as usize
                }
                _ => {
                    let until_gap = gap.map_or(u64::MAX, |(frame, _)| frame - self.cursor);
                    let n = available.min(WRITE_CHUNK).min(until_gap as usize);
                    if n == 0 {
                        return wrote;
                    }
                    self.read_frames(n, scratch);
                    available -= n;
                    n
                }
            };
            self.write(n, scratch);
            wrote = true;
        }
    }

    fn read_frames(&mut self, n: usize, scratch: &mut [f32]) {
        if let Ok(chunk) = self.frames.read_chunk(n * self.channels) {
            let (first, second) = chunk.as_slices();
            scratch[..first.len()].copy_from_slice(first);
            scratch[first.len()..first.len() + second.len()].copy_from_slice(second);
            chunk.commit_all();
        }
    }

    fn write(&mut self, n: usize, data: &[f32]) {
        let base = self.cursor;
        let end = base + n as u64;
        let (channels, files) = (self.channels, &mut self.files);
        self.splitter.advance(end, |span| {
            let from = (span.frames.start - base) as usize * channels;
            let to = (span.frames.end - base) as usize * channels;
            files.write(span.new_pass, span.start, &data[from..to], channels);
        });
        self.cursor = end;
    }

    fn finish(mut self) -> TakeReport {
        let dropped = self.shared.dropped.load(Ordering::Relaxed);
        let input = self.input.underruns() + self.input.overflows();
        self.files.close_pass();
        TakeReport {
            passes: std::mem::take(&mut self.files.passes),
            dropouts: dropped + input.saturating_sub(self.dropouts_before),
            latency: self.files.latency,
            error: self.files.error.take(),
        }
    }
}

struct TrackFiles {
    track: u64,
    name: String,
    /// Where the file's channels are in a captured frame.
    channels: Vec<usize>,
}

impl TrackFiles {
    fn new(track: &RecordTrack, sources: &[usize]) -> Self {
        let wanted = match track.route {
            InputRoute::Mono(c) => vec![c],
            InputRoute::Stereo(c) => vec![c, c + 1],
        };
        Self {
            track: track.track,
            name: track.name.clone(),
            channels: wanted
                .into_iter()
                .filter_map(|c| sources.iter().position(|s| *s == c))
                .collect(),
        }
    }
}

struct OpenPass {
    start: SamplePos,
    frames: u64,
    writers: Vec<(WavWriter, TakeFile)>,
}

/// The files of the pass being written, and of those before it.
struct PassFiles {
    directory: PathBuf,
    tracks: Vec<TrackFiles>,
    sample_rate: f64,
    min_frames: u64,
    open: Option<OpenPass>,
    passes: Vec<TakePass>,
    error: Option<String>,
    scratch: Vec<f32>,
    latency: u64,
}

impl PassFiles {
    fn write(&mut self, new_pass: bool, start: SamplePos, data: &[f32], channels: usize) {
        if self.error.is_some() {
            return;
        }
        if new_pass || self.open.is_none() {
            self.close_pass();
            if let Err(e) = self.open_pass(start) {
                self.fail(e);
                return;
            }
        }
        let Some(open) = &mut self.open else { return };
        let mut failed = None;
        for ((wav, _), track) in open.writers.iter_mut().zip(&self.tracks) {
            self.scratch.clear();
            for frame in data.chunks_exact(channels) {
                self.scratch
                    .extend(track.channels.iter().map(|c| frame[*c]));
            }
            if let Err(e) = wav.write_interleaved(&self.scratch) {
                failed = Some(e.to_string());
                break;
            }
        }
        open.frames += (data.len() / channels) as u64;
        if let Some(e) = failed {
            self.fail(e);
        }
    }

    fn open_pass(&mut self, start: SamplePos) -> Result<(), String> {
        let mut writers = Vec::with_capacity(self.tracks.len());
        for track in &self.tracks {
            let path = free_path(&self.directory, &track.name);
            let wav = WavWriter::create(&path, track.channels.len(), self.sample_rate)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let file = TakeFile {
                track: track.track,
                path,
            };
            writers.push((wav, file));
        }
        self.open = Some(OpenPass {
            start,
            frames: 0,
            writers,
        });
        Ok(())
    }

    fn fail(&mut self, error: String) {
        self.error = Some(error);
        self.close_pass();
    }

    fn flush(&mut self) {
        let Some(open) = &mut self.open else { return };
        let mut failed = None;
        for (wav, _) in &mut open.writers {
            if let Err(e) = wav.flush() {
                failed = Some(e.to_string());
            }
        }
        if let Some(e) = failed {
            self.fail(e);
        }
    }

    /// Finish the files of the open pass, and report it if it's long enough.
    fn close_pass(&mut self) {
        let Some(open) = self.open.take() else { return };
        let mut files = Vec::with_capacity(open.writers.len());
        for (wav, file) in open.writers {
            if let Err(e) = wav.finalize() {
                self.error.get_or_insert_with(|| e.to_string());
            }
            files.push(file);
        }
        if open.frames < self.min_frames {
            for file in &files {
                let _ = std::fs::remove_file(&file.path);
            }
            return;
        }
        self.passes.push(TakePass {
            start: open.start,
            frames: open.frames,
            files,
        });
    }
}

/// The first "`name` take N.wav" not taken yet.
fn free_path(directory: &std::path::Path, name: &str) -> PathBuf {
    (1u32..)
        .map(|n| directory.join(format!("{name} take {n}.wav")))
        .find(|path| !path.exists())
        .expect("fewer than 2^32 takes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::InputFeed;

    fn seg(frame: u64, pos: SamplePos, playing: bool) -> Segment {
        Segment {
            frame,
            pos,
            playing,
            loop_range: None,
        }
    }

    /// (first frame, frames, timeline start) of each pass, given frames in
    /// steps of `step`.
    fn passes(segments: &[Segment], latency: u64, end: u64, step: u64) -> Vec<(u64, u64, i64)> {
        let mut splitter = Splitter::new(latency);
        let mut pushed = 0;
        let mut out: Vec<(u64, u64, i64)> = Vec::new();
        let mut at = 0;
        while at < end {
            at = (at + step).min(end);
            // Known once their frame has been captured.
            while pushed < segments.len() && segments[pushed].frame <= at {
                splitter.push(segments[pushed]);
                pushed += 1;
            }
            splitter.advance(at, |span| {
                let frames = span.frames.end - span.frames.start;
                match out.last_mut() {
                    Some(last) if !span.new_pass => last.1 += frames,
                    _ => out.push((span.frames.start, frames, span.start)),
                }
            });
        }
        out
    }

    #[test]
    fn frames_come_back_latency_frames_after_they_were_played_to() {
        let log = [seg(100, 5000, true)];
        assert_eq!(passes(&log, 30, 1000, 64), [(130, 870, 5000)]);
    }

    #[test]
    fn frames_before_the_transport_rolls_are_dropped() {
        let log = [seg(0, 0, false), seg(500, 1000, true)];
        assert_eq!(passes(&log, 30, 1000, 64), [(530, 470, 1000)]);
    }

    #[test]
    fn stopping_ends_the_pass_once_it_has_come_back() {
        let log = [seg(0, 0, true), seg(500, 500, false)];
        assert_eq!(passes(&log, 30, 1000, 64), [(30, 500, 0)]);
    }

    #[test]
    fn a_seek_starts_a_new_pass() {
        let log = [seg(0, 0, true), seg(500, 9000, true)];
        assert_eq!(passes(&log, 30, 1000, 64), [(30, 500, 0), (530, 470, 9000)]);
    }

    #[test]
    fn a_change_that_is_no_jump_stays_in_the_pass() {
        let log = [seg(0, 0, true), seg(500, 500, true)];
        assert_eq!(passes(&log, 30, 1000, 64), [(30, 970, 0)]);
    }

    #[test]
    fn loop_wraps_make_passes_and_the_last_one_can_be_partial() {
        let looped = |frame, pos| Segment {
            loop_range: Some((1000, 1400)),
            ..seg(frame, pos, true)
        };
        let log = [looped(0, 1100), looped(300, 1000), looped(700, 1000)];
        assert_eq!(
            passes(&log, 50, 1000, 64),
            [(50, 300, 1100), (350, 400, 1000), (750, 250, 1000)]
        );
    }

    #[test]
    fn only_what_is_inside_the_loop_gets_recorded() {
        let looped = |frame, pos| Segment {
            loop_range: Some((1000, 1400)),
            ..seg(frame, pos, true)
        };
        // Rolling from before the loop.
        let log = [looped(0, 800), looped(600, 1000)];
        assert_eq!(
            passes(&log, 0, 1000, 64),
            [(200, 400, 1000), (600, 400, 1000)]
        );
        // Past its end.
        assert_eq!(passes(&[looped(0, 2000)], 0, 1000, 64), []);
    }

    #[test]
    fn the_steps_frames_arrive_in_make_no_difference() {
        let log = [seg(0, 0, true), seg(500, 9000, true), seg(700, 700, false)];
        let expected = passes(&log, 30, 1000, 1000);
        for step in [1, 7, 64, 333] {
            assert_eq!(passes(&log, 30, 1000, step), expected, "step {step}");
        }
        assert_eq!(expected, [(30, 500, 0), (530, 200, 9000)]);
    }

    #[test]
    fn sources_are_the_channels_tracks_use_once_each() {
        let track = |route| RecordTrack {
            track: 0,
            route,
            name: String::new(),
        };
        let tracks = [
            track(InputRoute::Stereo(2)),
            track(InputRoute::Mono(3)),
            track(InputRoute::Mono(0)),
        ];
        assert_eq!(&*sources_of(&tracks), [0, 2, 3]);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tonique-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Frames that don't fit in the ring stay on the timeline, as silence.
    #[test]
    fn dropped_frames_are_counted_and_keep_the_take_in_place() {
        let dir = temp_dir("record-gap");
        let (mut feed, mut input) = InputFeed::new(1, 8, 1);
        let status = feed.status();
        let options = RecordOptions {
            directory: dir.clone(),
            tracks: vec![RecordTrack {
                track: 7,
                route: InputRoute::Mono(0),
                name: "t".into(),
            }],
            latency_offset_frames: 0,
            count_in: None,
        };
        let sources = sources_of(&options.tracks);
        let (mut capture, mut writer) = new_take(options, sources, 16, 0, 48000.0, status);
        writer.files.min_frames = 0;

        // The ring holds 2 chunks: the writer isn't running yet.
        let mut next = 1;
        let chunks = 4;
        for chunk in 0..chunks {
            input.push((0..8).map(|i| (next + i) as f32));
            next += 8;
            let view = feed.pull(8);
            capture.record(view, 8, chunk * 8, true, None);
        }
        // Room again, for one last chunk after the gap.
        let (tx, rx) = std::sync::mpsc::channel();
        let (mut scratch, available) = (vec![0.0; 4096], writer.frames.slots());
        while let Ok(event) = writer.events.pop() {
            if let CaptureEvent::Segment(s) = event {
                writer.splitter.push(s);
            }
        }
        writer.write_available(available, &mut scratch);
        input.push((0..8).map(|i| (next + i) as f32));
        let view = feed.pull(8);
        capture.record(view, 8, chunks * 8, true, None);
        capture.stop();
        capture.record(InputView::NONE, 8, 0, false, None);
        writer.run(tx);

        let report = rx.recv().unwrap();
        assert_eq!(report.dropouts, 16);
        assert_eq!(report.passes.len(), 1);
        assert_eq!(report.passes[0].frames, 40);
        let wav = crate::sample::SampleBuffer::load_wav(&report.passes[0].files[0].path, 48000.0);
        let wav = wav.unwrap();
        let samples = wav.channel(0);
        assert_eq!(samples[15], 16.0);
        assert!(samples[16..32].iter().all(|s| *s == 0.0), "{samples:?}");
        assert_eq!(samples[32], 33.0, "after the gap, in its own place");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_count_in_clicks_each_beat_for_its_bars() {
        let tempo = TempoMap::new(120.0);
        let mut count_in = CountIn::new(&tempo, BeatPos(0.0), 2, 48000.0, 1.0).unwrap();
        assert_eq!(count_in.remaining(), 8 * 24000);
        let mut out = vec![0.0; 8 * 24000 * 2];
        count_in.mix(&mut out, 2);
        assert_eq!(count_in.remaining(), 0);
        for beat in 0..8 {
            let at = beat * 24000 * 2;
            assert_eq!(out[at], 0.0, "starts from silence");
            assert!(out[at + 2] != 0.0, "beat {beat}");
            assert_eq!(out[at + 2], out[at + 3], "both sides");
        }
        assert!(out[12000 * 2] == 0.0, "silent between beats");
    }

    #[test]
    fn a_count_in_of_no_bars_is_none() {
        let tempo = TempoMap::new(120.0);
        assert!(CountIn::new(&tempo, BeatPos(0.0), 0, 48000.0, 1.0).is_none());
    }

    #[test]
    fn a_count_in_follows_the_time_signature_at_its_start() {
        let mut tempo = TempoMap::new(120.0);
        tempo.time_signature = crate::time::TimeSignature::new(6, 8).unwrap();
        let count_in = CountIn::new(&tempo, BeatPos(0.0), 1, 48000.0, 1.0).unwrap();
        assert_eq!(count_in.clicks.len(), 6);
        assert_eq!(count_in.clicks[1], (12000, false));
        assert_eq!(count_in.remaining(), 72000);
    }

    #[test]
    fn takes_get_the_first_free_name() {
        let dir = temp_dir("record-names");
        assert_eq!(free_path(&dir, "Vox"), dir.join("Vox take 1.wav"));
        std::fs::write(dir.join("Vox take 1.wav"), b"").unwrap();
        assert_eq!(free_path(&dir, "Vox"), dir.join("Vox take 2.wav"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
