//! The engine: a control-side [`Engine`] and an RT-side [`AudioProcessor`].
//!
//! They only talk through lock-free SPSC rings and atomics:
//!
//! ```text
//!   Engine (UI / control)                AudioProcessor (audio callback)
//!   ─────────────────────                ───────────────────────────────
//!   compile() ── Box<CompiledGraph> ──▶  adopt at top of block, migrate
//!                                        node state by identity
//!   play/seek/… ── Command ──────────▶  drained at top of block
//!   housekeeping ◀── Garbage ─────────  old graphs, swapped-out curves
//!   position()   ◀── atomics ─────────  transport position, stats
//!   input callback ── InputFeed ring ─▶  pulled per chunk (see [`crate::input`])
//!   take writer ◀── Capture rings ─────  input and transport log (see [`crate::record`])
//! ```
//!
//! The processor never frees memory: everything it lets go of goes back
//! through the garbage ring and is dropped by a housekeeping thread.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::graph::scheduler::WorkerPool;
use crate::graph::{
    BlockInfo, CompileError, CompileOptions, CompileStats, CompiledGraph, GraphDescription,
    GraphTopology, NodeIdentity, NodeMessage, compile,
};
use crate::input::{InputFeed, InputStatus, InputView};
use crate::preview::{PreviewControl, PreviewShared, PreviewSource, PreviewStream};
use crate::record::{self, Capture, CountIn, RecordOptions, TakeReport};
use crate::rt;
use crate::time::{BeatPos, SamplePos, TempoMap};

#[derive(Clone, Copy, Debug)]
pub struct EngineConfig {
    pub sample_rate: f64,
    /// Largest block processed at once; device buffers are split into these.
    pub max_block: usize,
    pub output_channels: usize,
    /// Helper threads for parallel graph processing (0 = single-threaded).
    pub worker_threads: usize,
    /// Graphs with fewer nodes run sequentially: below this, coordination
    /// costs more than it saves.
    pub parallel_threshold: usize,
    /// Spawn a thread that frees retired objects. If false, call
    /// [`Engine::collect_garbage`] yourself (useful for offline rendering).
    pub housekeeping_thread: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000.0,
            max_block: 512,
            output_channels: 2,
            worker_threads: 0,
            parallel_threshold: 32,
            housekeeping_thread: true,
        }
    }
}

/// Commands from control threads to the RT thread.
#[derive(Debug)]
pub enum Command {
    Play,
    Stop,
    Seek(SamplePos),
    /// Seek to a beat, placed with the tempo of the graph playing (see
    /// [`crate::graph::GraphDescription::set_tempo`]): whatever graph
    /// comes before or after, it lands on that beat.
    SeekBeats(BeatPos),
    /// Loop `[start, end)` while playing; `None` disables looping.
    SetLoop(Option<(SamplePos, SamplePos)>),
    /// Loop between two beats, as [`Command::SeekBeats`]: they stay where
    /// they are in beats when the tempo changes.
    SetLoopBeats(Option<(BeatPos, BeatPos)>),
    /// Deliver a message to the node with this identity (e.g. swap in a new
    /// automation curve without rebuilding the graph).
    SendToNode {
        target: NodeIdentity,
        msg: NodeMessage,
    },
    /// Replace the preview stream (`None` stops the preview).
    SetPreview {
        stream: Option<PreviewStream>,
        generation: u64,
    },
    /// Replace the input (`None` removes it).
    SetInput(Option<Box<InputFeed>>),
    /// Start capturing a take (see [`Engine::record_start`]).
    RecordStart(Box<RecordStart>),
    /// Punch out, once what was played has come back.
    RecordStop,
}

/// What [`Command::RecordStart`] hands over.
#[derive(Debug)]
pub struct RecordStart {
    capture: Option<Box<Capture>>,
    count_in: Option<Box<CountIn>>,
}

/// Things the RT thread has let go of, to be dropped elsewhere.
pub enum Garbage {
    Graph(Box<CompiledGraph>),
    Message(NodeMessage),
    Command(Command),
    Capture(Box<Capture>),
    CountIn(Box<CountIn>),
}

#[derive(Debug)]
pub enum EngineError {
    Compile(CompileError),
    CommandQueueFull,
    Incompatible(&'static str),
    /// A take couldn't start.
    Record(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compile(e) => write!(f, "compile error: {e}"),
            Self::CommandQueueFull => write!(f, "command queue full"),
            Self::Incompatible(why) => write!(f, "incompatible graph: {why}"),
            Self::Record(why) => write!(f, "can't record: {why}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<CompileError> for EngineError {
    fn from(e: CompileError) -> Self {
        Self::Compile(e)
    }
}

/// State published by the RT thread for the UI to read.
#[derive(Default)]
struct Shared {
    position: AtomicI64,
    /// The position in beats, through the playing graph's tempo map
    /// (`f64` bits; NaN without one).
    position_beats: AtomicU64,
    playing: AtomicBool,
    graphs_adopted: AtomicU64,
    blocks: AtomicU64,
    /// Nodes whose state was carried over by the last adoption.
    last_migrated: AtomicU64,
    /// Time spent in the last callback relative to its real-time budget, in 1/1000.
    load_permille: AtomicU64,
    /// Frames from a callback rendering audio to the device playing it.
    output_latency: AtomicUsize,
    counting_in: AtomicBool,
}

/// Graphs on their way to the RT thread. If the ring is full (the audio
/// thread is stalled, or edits arrive faster than blocks), only the newest
/// graph is kept: intermediate graphs are pointless because state migrates
/// by identity from whatever graph is actually running.
struct GraphOutbox {
    tx: rtrb::Producer<Box<CompiledGraph>>,
    pending: Option<Box<CompiledGraph>>,
}

impl GraphOutbox {
    fn flush(&mut self) {
        if let Some(g) = self.pending.take()
            && let Err(rtrb::PushError::Full(g)) = self.tx.push(g)
        {
            self.pending = Some(g);
        }
    }
}

/// Control-side handle. Lives on the UI / control thread.
pub struct Engine {
    config: EngineConfig,
    commands: rtrb::Producer<Command>,
    graphs: Arc<Mutex<GraphOutbox>>,
    shared: Arc<Shared>,
    garbage: Arc<Mutex<rtrb::Consumer<Garbage>>>,
    housekeeping: Option<(JoinHandle<()>, Arc<AtomicBool>)>,
    preview: PreviewControl,
    /// Structure of the latest published graph.
    topology: Option<Arc<GraphTopology>>,
    graph_metering: bool,
    /// Latency of the output of the latest published graph.
    graph_latency: usize,
    input: Option<Arc<InputStatus>>,
    /// Where the report of the take being written comes from.
    take: Option<Receiver<TakeReport>>,
}

/// RT-side processor. Move it into the audio callback; call
/// [`AudioProcessor::process_interleaved`] once per device buffer.
pub struct AudioProcessor {
    config: EngineConfig,
    commands: rtrb::Consumer<Command>,
    graphs: rtrb::Consumer<Box<CompiledGraph>>,
    garbage: rtrb::Producer<Garbage>,
    shared: Arc<Shared>,
    current: Option<Box<CompiledGraph>>,
    pool: Option<WorkerPool>,
    playing: bool,
    position: SamplePos,
    loop_range: Option<(SamplePos, SamplePos)>,
    /// The loop in beats, when set so: placed again with each tempo.
    loop_beats: Option<(BeatPos, BeatPos)>,
    /// A seek in beats waiting for a graph with a tempo map to place it.
    seek_beats: Option<BeatPos>,
    /// The transport was inside the loop when the tempo last changed: it
    /// stays inside, wherever rounding put it and the loop's end.
    retimed_in_loop: bool,
    jumped: bool,
    preview: Option<PreviewStream>,
    preview_generation: u64,
    preview_shared: Arc<PreviewShared>,
    input: Option<Box<InputFeed>>,
    capture: Option<Box<Capture>>,
    count_in: Option<Box<CountIn>>,
}

const COMMAND_CAPACITY: usize = 1024;
const GRAPH_CAPACITY: usize = 8;
const GARBAGE_CAPACITY: usize = 2048;
/// Upper bound on commands handled per callback (bounded work).
const MAX_COMMANDS_PER_BLOCK: usize = 256;

impl Engine {
    pub fn new(config: EngineConfig) -> (Engine, AudioProcessor) {
        let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(COMMAND_CAPACITY);
        let (graph_tx, graph_rx) = rtrb::RingBuffer::new(GRAPH_CAPACITY);
        let (garbage_tx, garbage_rx) = rtrb::RingBuffer::new(GARBAGE_CAPACITY);
        let shared = Arc::new(Shared::default());
        // No graph has placed the position in beats yet.
        shared
            .position_beats
            .store(f64::NAN.to_bits(), Ordering::Relaxed);
        let garbage = Arc::new(Mutex::new(garbage_rx));
        let graphs = Arc::new(Mutex::new(GraphOutbox {
            tx: graph_tx,
            pending: None,
        }));

        let housekeeping = config.housekeeping_thread.then(|| {
            let stop = Arc::new(AtomicBool::new(false));
            let (g, s, outbox) = (garbage.clone(), stop.clone(), graphs.clone());
            let handle = std::thread::Builder::new()
                .name("tonique-housekeeping".into())
                .spawn(move || {
                    while !s.load(Ordering::Relaxed) {
                        lock(&outbox).flush();
                        drain_garbage(&g);
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    drain_garbage(&g);
                })
                .expect("spawn housekeeping thread");
            (handle, stop)
        });

        let pool = (config.worker_threads > 0).then(|| WorkerPool::new(config.worker_threads));
        let preview = PreviewControl::new(config.sample_rate);
        let processor = AudioProcessor {
            config,
            commands: cmd_rx,
            graphs: graph_rx,
            garbage: garbage_tx,
            shared: shared.clone(),
            current: None,
            pool,
            playing: false,
            position: 0,
            loop_range: None,
            loop_beats: None,
            seek_beats: None,
            retimed_in_loop: false,
            jumped: true,
            preview: None,
            preview_generation: 0,
            preview_shared: preview.shared(),
            input: None,
            capture: None,
            count_in: None,
        };
        let engine = Engine {
            config,
            commands: cmd_tx,
            graphs,
            shared,
            garbage,
            housekeeping,
            preview,
            topology: None,
            graph_metering: false,
            graph_latency: 0,
            input: None,
            take: None,
        };
        (engine, processor)
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub fn compile_options(&self) -> CompileOptions {
        CompileOptions {
            sample_rate: self.config.sample_rate,
            max_block: self.config.max_block,
        }
    }

    /// Compile off-thread (here, on the caller's thread) and hand the result
    /// to the audio thread. It takes effect at the start of the next block.
    pub fn load_graph(&mut self, desc: GraphDescription) -> Result<CompileStats, EngineError> {
        let graph = compile(desc, &self.compile_options())?;
        let stats = graph.stats();
        self.publish(graph)?;
        Ok(stats)
    }

    pub fn publish(&mut self, graph: CompiledGraph) -> Result<(), EngineError> {
        if graph.sample_rate() != self.config.sample_rate {
            return Err(EngineError::Incompatible("sample rate differs from engine"));
        }
        if graph.max_block() < self.config.max_block {
            return Err(EngineError::Incompatible("max block smaller than engine's"));
        }
        self.graph_latency = graph.stats().output_latency;
        let topology = graph.topology();
        topology.meters().set_enabled(self.graph_metering);
        self.topology = Some(topology);
        let mut outbox = lock(&self.graphs);
        outbox.flush();
        // Replacing a still-pending graph drops it here, on this thread.
        outbox.pending = Some(Box::new(graph));
        outbox.flush();
        Ok(())
    }

    /// Whether a published graph is still waiting for the audio thread.
    pub fn has_pending_graph(&self) -> bool {
        let mut outbox = lock(&self.graphs);
        outbox.flush();
        outbox.pending.is_some()
    }

    pub fn send(&mut self, cmd: Command) -> Result<(), EngineError> {
        self.commands
            .push(cmd)
            .map_err(|_| EngineError::CommandQueueFull)
    }

    pub fn play(&mut self) -> Result<(), EngineError> {
        self.send(Command::Play)
    }

    pub fn stop(&mut self) -> Result<(), EngineError> {
        self.send(Command::Stop)
    }

    pub fn seek(&mut self, pos: SamplePos) -> Result<(), EngineError> {
        self.send(Command::Seek(pos))
    }

    /// See [`Command::SeekBeats`].
    pub fn seek_beats(&mut self, beat: BeatPos) -> Result<(), EngineError> {
        self.send(Command::SeekBeats(beat))
    }

    pub fn set_loop(&mut self, range: Option<(SamplePos, SamplePos)>) -> Result<(), EngineError> {
        self.send(Command::SetLoop(range))
    }

    /// See [`Command::SetLoopBeats`].
    pub fn set_loop_beats(&mut self, range: Option<(BeatPos, BeatPos)>) -> Result<(), EngineError> {
        self.send(Command::SetLoopBeats(range))
    }

    pub fn position(&self) -> SamplePos {
        self.shared.position.load(Ordering::Relaxed)
    }

    /// The position in beats, as the audio thread places it with the tempo
    /// it plays at: `None` until a graph with a tempo map plays.
    pub fn position_beats(&self) -> Option<BeatPos> {
        let beats = f64::from_bits(self.shared.position_beats.load(Ordering::Relaxed));
        (!beats.is_nan()).then_some(BeatPos(beats))
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    pub fn graphs_adopted(&self) -> u64 {
        self.shared.graphs_adopted.load(Ordering::Relaxed)
    }

    /// How many node instances kept their state in the last graph swap.
    pub fn last_migrated(&self) -> u64 {
        self.shared.last_migrated.load(Ordering::Relaxed)
    }

    pub fn blocks_processed(&self) -> u64 {
        self.shared.blocks.load(Ordering::Relaxed)
    }

    /// Last callback's processing time as a fraction of its real-time budget.
    pub fn cpu_load(&self) -> f32 {
        self.shared.load_permille.load(Ordering::Relaxed) as f32 / 1000.0
    }

    /// Structure of the latest published graph (the one playing, or about
    /// to within a block), with its live meters.
    pub fn graph_topology(&self) -> Option<Arc<GraphTopology>> {
        self.topology.clone()
    }

    /// Measure each node's output peak and processing time (see
    /// [`GraphTopology::meters`]). Off by default: it costs a little time
    /// per node on the audio thread.
    pub fn set_graph_metering(&mut self, on: bool) {
        self.graph_metering = on;
        if let Some(t) = &self.topology {
            t.meters().set_enabled(on);
        }
    }

    /// Play `source` from source frame `from`, alongside (and independent
    /// of) the transport. Replaces any preview already playing.
    pub fn preview_play(
        &mut self,
        source: Box<dyn PreviewSource>,
        from: usize,
    ) -> Result<(), EngineError> {
        let (stream, generation) = self.preview.play(source, from);
        self.send(Command::SetPreview {
            stream: Some(stream),
            generation,
        })
    }

    /// Continue the last previewed source from `from` (does nothing if
    /// nothing was previewed yet).
    pub fn preview_seek(&mut self, from: usize) -> Result<(), EngineError> {
        match self.preview.seek(from) {
            Some((stream, generation)) => self.send(Command::SetPreview {
                stream: Some(stream),
                generation,
            }),
            None => Ok(()),
        }
    }

    pub fn preview_stop(&mut self) -> Result<(), EngineError> {
        let generation = self.preview.stop();
        self.send(Command::SetPreview {
            stream: None,
            generation,
        })
    }

    /// Linear gain of the preview, kept across plays.
    pub fn preview_set_gain(&mut self, gain: f32) {
        self.preview.set_gain(gain);
    }

    /// True from `preview_play`/`preview_seek` until stopped or played to the end.
    pub fn is_previewing(&self) -> bool {
        self.preview.is_playing()
    }

    /// Preview position in frames of the source, while previewing.
    pub fn preview_position(&self) -> Option<usize> {
        self.preview.position()
    }

    /// Feed nodes from `feed` (see [`crate::input`]), replacing any input.
    /// `None` removes it. The old feed is freed off the audio thread.
    pub fn set_input(&mut self, feed: Option<InputFeed>) -> Result<(), EngineError> {
        if feed
            .as_ref()
            .is_some_and(|f| f.max_block() < self.config.max_block)
        {
            return Err(EngineError::Incompatible(
                "input feed's max block smaller than engine's",
            ));
        }
        self.input = feed.as_ref().map(InputFeed::status);
        self.send(Command::SetInput(feed.map(Box::new)))
    }

    /// Start capturing the input, from the next block, into new WAV files
    /// next to each other: one per track and per pass over the timeline
    /// (see [`crate::record`]). Doesn't touch the transport, unless with a
    /// count-in from stopped, which starts it at the end.
    ///
    /// Takes are moved back by the audio's round trip, fixed here: the
    /// input's latency and ring, the output's, the graph's, and the offset.
    pub fn record_start(&mut self, options: RecordOptions) -> Result<(), EngineError> {
        if self.take.is_some() {
            return Err(EngineError::Record("already recording".into()));
        }
        if options.tracks.is_empty() {
            return Err(EngineError::Record("no tracks".into()));
        }
        let Some(input) = self.input.clone() else {
            return Err(EngineError::Record("no input".into()));
        };
        std::fs::create_dir_all(&options.directory)
            .map_err(|e| EngineError::Record(format!("{}: {e}", options.directory.display())))?;
        let latency = input.latency_frames() as i64
            + input.mean_fill_frames() as i64
            + self.output_latency_frames() as i64
            + self.graph_latency as i64
            + options.latency_offset_frames;
        let sample_rate = self.config.sample_rate;
        let sources = record::sources_of(&options.tracks);
        let mut options = options;
        let count_in = options.count_in.take().map(Box::new);
        let (capture, writer) = record::new_take(
            options,
            sources,
            record::ring_frames(sample_rate),
            latency.max(0) as u64,
            sample_rate,
            input,
        );
        let (report, take) = channel();
        std::thread::Builder::new()
            .name("tonique-takes".into())
            .spawn(move || writer.run(report))
            .map_err(|e| EngineError::Record(e.to_string()))?;
        self.send(Command::RecordStart(Box::new(RecordStart {
            capture: Some(Box::new(capture)),
            count_in,
        })))?;
        self.take = Some(take);
        Ok(())
    }

    /// Punch out, and cancel a count-in. The take's report follows once
    /// what was played has come back: see [`Self::take_report`]. The
    /// transport goes on.
    pub fn record_stop(&mut self) -> Result<(), EngineError> {
        self.send(Command::RecordStop)
    }

    /// Whether a take is being written, from [`Self::record_start`] until
    /// its report has been picked up.
    pub fn is_recording(&self) -> bool {
        self.take.is_some()
    }

    /// Whether a count-in is playing.
    pub fn is_counting_in(&self) -> bool {
        self.shared.counting_in.load(Ordering::Relaxed)
    }

    /// The report of the take just finished, if there's one to pick up.
    pub fn take_report(&mut self) -> Option<TakeReport> {
        let report = match self.take.as_ref()?.try_recv() {
            Ok(report) => report,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => TakeReport {
                error: Some("the take writer stopped".into()),
                ..Default::default()
            },
        };
        self.take = None;
        Some(report)
    }

    /// Frames from the audio thread rendering audio to the output device
    /// playing it, as last reported by the device. 0 without one.
    pub fn output_latency_frames(&self) -> usize {
        self.shared.output_latency.load(Ordering::Relaxed)
    }

    /// Free everything the RT thread has retired so far, and hand over any
    /// pending graph. Called periodically by the housekeeping thread.
    pub fn collect_garbage(&self) -> usize {
        lock(&self.graphs).flush();
        drain_garbage(&self.garbage)
    }
}

/// Control-side only; the RT thread never touches these mutexes.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn drain_garbage(g: &Mutex<rtrb::Consumer<Garbage>>) -> usize {
    let mut rx = lock(g);
    let mut n = 0;
    while let Ok(item) = rx.pop() {
        drop(item);
        n += 1;
    }
    n
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some((handle, stop)) = self.housekeeping.take() {
            stop.store(true, Ordering::Relaxed);
            let _ = handle.join();
        }
    }
}

impl AudioProcessor {
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub fn graph(&self) -> Option<&CompiledGraph> {
        self.current.as_deref()
    }

    /// Report the output device's latency (see
    /// [`Engine::output_latency_frames`]).
    pub fn set_output_latency_frames(&self, frames: usize) {
        self.shared.output_latency.store(frames, Ordering::Relaxed);
    }

    /// Render one device buffer. `out` is interleaved with `channels`
    /// channels. Allocation-free, lock-free and bounded.
    pub fn process_interleaved(&mut self, out: &mut [f32], channels: usize) {
        let started = Instant::now();
        rt::no_alloc(|| {
            self.handle_commands();
            self.adopt_graphs();
            self.place_beats();
            self.keep_in_loop();
            if let Some(feed) = &mut self.input {
                feed.begin_callback();
            }
            let frames = out.len() / channels;
            let mut done = 0;
            while done < frames {
                let mut n = (frames - done).min(self.config.max_block);
                if let Some(count_in) = &self.count_in
                    && count_in.remaining() > 0
                {
                    n = n.min(count_in.remaining().min(usize::MAX as u64) as usize);
                }
                if let (true, Some((_, le))) = (self.playing, self.loop_range)
                    && self.position < le
                    && self.position + n as SamplePos > le
                {
                    n = (le - self.position) as usize; // split exactly at the loop end
                }
                self.render_chunk(
                    &mut out[done * channels..(done + n) * channels],
                    channels,
                    n,
                );
                self.mix_count_in(&mut out[done * channels..(done + n) * channels], channels);
                done += n;
            }
            self.mix_preview(out, channels);
            self.shared.position.store(self.position, Ordering::Relaxed);
            let beats = match self.tempo() {
                Some(tempo) => {
                    tempo
                        .samples_to_beats(self.position, self.config.sample_rate)
                        .0
                }
                None => f64::NAN,
            };
            self.shared
                .position_beats
                .store(beats.to_bits(), Ordering::Relaxed);
            self.shared.playing.store(self.playing, Ordering::Relaxed);
            self.shared
                .counting_in
                .store(self.count_in.is_some(), Ordering::Relaxed);
            self.shared.blocks.fetch_add(1, Ordering::Relaxed);
        });
        let budget = frames_to_secs(out.len() / channels.max(1), self.config.sample_rate);
        if budget > 0.0 {
            let load = started.elapsed().as_secs_f64() / budget;
            self.shared
                .load_permille
                .store((load * 1000.0) as u64, Ordering::Relaxed);
        }
    }

    /// Add the count-in's click to a chunk, and roll at its end.
    fn mix_count_in(&mut self, out: &mut [f32], channels: usize) {
        let Some(count_in) = &mut self.count_in else {
            return;
        };
        count_in.mix(out, channels);
        if count_in.remaining() == 0 && self.garbage.slots() > 0 {
            self.playing = true;
            self.jumped = true;
            self.retire_count_in();
        }
    }

    fn retire_count_in(&mut self) {
        if self.garbage.slots() > 0
            && let Some(count_in) = self.count_in.take()
        {
            let _ = self.garbage.push(Garbage::CountIn(count_in));
        }
    }

    /// Add queued preview frames to the first two channels of `out`.
    fn mix_preview(&mut self, out: &mut [f32], channels: usize) {
        let Some(stream) = &mut self.preview else {
            return;
        };
        let frames = out.len() / channels.max(1);
        let n = stream.slots().min(frames);
        let gain = f32::from_bits(self.preview_shared.gain.load(Ordering::Relaxed));
        if let Ok(chunk) = stream.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            for (frame, [l, r]) in out.chunks_exact_mut(channels).zip(a.iter().chain(b)) {
                frame[0] += l * gain;
                if channels > 1 {
                    frame[1] += r * gain;
                }
            }
            chunk.commit_all();
        }
        self.preview_shared
            .played
            .fetch_add(n as u64, Ordering::Relaxed);
        // The feeder abandons the ring at the end of the source.
        if stream.is_empty() && stream.is_abandoned() {
            self.preview_shared
                .finished
                .store(self.preview_generation, Ordering::Relaxed);
        }
    }

    fn render_chunk(&mut self, out: &mut [f32], channels: usize, n: usize) {
        let input = match &mut self.input {
            Some(feed) => feed.pull(n),
            None => InputView::NONE,
        };
        if let Some(capture) = &mut self.capture {
            capture.record(input, n, self.position, self.playing, self.loop_range);
            if capture.is_finished()
                && self.garbage.slots() > 0
                && let Some(capture) = self.capture.take()
            {
                let _ = self.garbage.push(Garbage::Capture(capture));
            }
        }
        let info = BlockInfo {
            block_len: n,
            sample_rate: self.config.sample_rate,
            timeline_pos: self.position,
            playing: self.playing,
            jumped: self.jumped,
            input,
        };
        self.jumped = false;
        match self.current.as_deref_mut() {
            Some(graph) => {
                match &self.pool {
                    Some(pool) if graph.len() >= self.config.parallel_threshold => {
                        pool.run_block(graph, &info)
                    }
                    _ => graph.process_sequential(&info),
                }
                let block = graph.output(n);
                for (f, frame) in out.chunks_exact_mut(channels).enumerate() {
                    for (c, s) in frame.iter_mut().enumerate() {
                        *s = if block.channels() == 0 {
                            0.0
                        } else {
                            block.channel(c.min(block.channels() - 1))[f]
                        };
                    }
                }
            }
            None => out.fill(0.0),
        }
        if self.playing {
            self.position += n as SamplePos;
            if let Some((ls, le)) = self.loop_range
                && self.position == le
            {
                self.position = ls;
                self.jumped = true;
            }
        }
    }

    fn handle_commands(&mut self) {
        for _ in 0..MAX_COMMANDS_PER_BLOCK {
            // Keep room to retire whatever the command leaves behind.
            if self.garbage.slots() == 0 {
                return;
            }
            let Ok(cmd) = self.commands.pop() else { return };
            match cmd {
                Command::Play => {
                    self.retire_count_in();
                    if !self.playing {
                        self.playing = true;
                        self.jumped = true;
                    }
                }
                Command::Stop => {
                    self.retire_count_in();
                    self.playing = false;
                }
                Command::Seek(pos) => {
                    self.position = pos;
                    self.seek_beats = None;
                    self.jumped = true;
                }
                // Placed once a graph's tempo is known (see `place_beats`).
                Command::SeekBeats(beat) => self.seek_beats = Some(beat),
                Command::SetLoop(range) => {
                    self.loop_range = range.filter(|(s, e)| e > s);
                    self.loop_beats = None;
                }
                Command::SetLoopBeats(range) => {
                    self.loop_beats = range.filter(|(s, e)| e.0 > s.0);
                    if self.loop_beats.is_none() {
                        self.loop_range = None;
                    }
                }
                Command::SendToNode { target, mut msg } => {
                    if let Some(g) = self.current.as_deref_mut() {
                        g.deliver(target, &mut msg);
                    }
                    let _ = self.garbage.push(Garbage::Message(msg));
                }
                Command::SetPreview {
                    mut stream,
                    generation,
                } => {
                    std::mem::swap(&mut self.preview, &mut stream);
                    self.preview_generation = generation;
                    self.preview_shared.played.store(0, Ordering::Relaxed);
                    self.preview_shared
                        .generation
                        .store(generation, Ordering::Relaxed);
                    // The old stream is freed off the RT thread.
                    let _ = self
                        .garbage
                        .push(Garbage::Command(Command::SetPreview { stream, generation }));
                }
                Command::SetInput(mut feed) => {
                    std::mem::swap(&mut self.input, &mut feed);
                    // The old feed is freed off the RT thread.
                    let _ = self.garbage.push(Garbage::Command(Command::SetInput(feed)));
                }
                Command::RecordStart(mut start) => {
                    if self.playing {
                        // Nothing to count in to: it's freed with the rest.
                    } else {
                        std::mem::swap(&mut self.count_in, &mut start.count_in);
                    }
                    std::mem::swap(&mut self.capture, &mut start.capture);
                    // Whatever this replaces is freed off the RT thread.
                    let _ = self
                        .garbage
                        .push(Garbage::Command(Command::RecordStart(start)));
                }
                Command::RecordStop => {
                    self.retire_count_in();
                    if let Some(capture) = &mut self.capture {
                        capture.stop();
                    }
                }
            }
        }
    }

    fn adopt_graphs(&mut self) {
        // Adopt every pending graph in order, migrating state each time, so
        // instances flow forward even through graphs that never played.
        while self.garbage.slots() > 0 {
            let Ok(mut next) = self.graphs.pop() else {
                return;
            };
            let migrated = match self.current.take() {
                Some(mut old) => {
                    if let (Some(from), Some(to)) = (old.tempo(), next.tempo())
                        && from != to
                    {
                        self.retime(from, to);
                    }
                    let m = next.adopt_state_from(&mut old);
                    let _ = self.garbage.push(Garbage::Graph(old));
                    m
                }
                None => 0,
            };
            self.current = Some(next);
            self.shared
                .last_migrated
                .store(migrated as u64, Ordering::Relaxed);
            self.shared.graphs_adopted.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl AudioProcessor {
    /// The tempo map of the graph playing, if it has one.
    fn tempo(&self) -> Option<&TempoMap> {
        self.current.as_deref().and_then(CompiledGraph::tempo)
    }

    /// The tempo changes from `from` to `to`: the transport (and a loop set
    /// in samples) keeps its place in beats, where its samples now fall
    /// elsewhere. Not a jump: what plays goes on from the same beat.
    fn retime(&mut self, from: &TempoMap, to: &TempoMap) {
        let sr = self.config.sample_rate;
        let move_to = |pos: SamplePos| to.beats_to_samples(from.samples_to_beats(pos, sr), sr);
        self.retimed_in_loop |= self
            .loop_range
            .is_some_and(|(s, e)| (s..e).contains(&self.position));
        self.position = move_to(self.position);
        if self.loop_beats.is_none() {
            self.loop_range = self.loop_range.map(|(s, e)| (move_to(s), move_to(e)));
        }
    }

    /// Rounded apart, the transport and the loop's end, moved to a new
    /// tempo, can land on or past each other, where the loop never ends:
    /// it goes round now instead.
    fn keep_in_loop(&mut self) {
        if !std::mem::take(&mut self.retimed_in_loop) {
            return;
        }
        if let Some((start, end)) = self.loop_range
            && self.position >= end
        {
            self.position = start;
            self.jumped = true;
        }
    }

    /// Place what was asked in beats (a seek, the loop) with the tempo of
    /// the graph playing, once there is one.
    fn place_beats(&mut self) {
        let Some(tempo) = self.current.as_deref().and_then(CompiledGraph::tempo) else {
            return;
        };
        let sr = self.config.sample_rate;
        if let Some(beat) = self.seek_beats.take() {
            self.position = tempo.beats_to_samples(beat, sr);
            self.jumped = true;
            self.retimed_in_loop = false;
        }
        if let Some((start, end)) = self.loop_beats {
            self.loop_range = Some((
                tempo.beats_to_samples(start, sr),
                tempo.beats_to_samples(end, sr),
            ))
            .filter(|(s, e)| e > s);
        }
    }
}

fn frames_to_secs(frames: usize, sr: f64) -> f64 {
    frames as f64 / sr
}

/// Render `frames` frames offline into an interleaved buffer.
pub fn render_offline(processor: &mut AudioProcessor, frames: usize, channels: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * channels];
    let block = processor.config.max_block;
    for chunk in out.chunks_mut(block * channels) {
        processor.process_interleaved(chunk, channels);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Node, NodeProperties, ProcessContext};

    /// Outputs the timeline position of each sample (when playing).
    struct Clock;
    impl Node for Clock {
        fn properties(&self) -> NodeProperties {
            NodeProperties::audio(1)
        }
        fn process(&mut self, ctx: &mut ProcessContext) {
            if ctx.playing {
                for (i, s) in ctx.audio_out.channel_mut(0).iter_mut().enumerate() {
                    *s = (ctx.timeline_pos + i as i64) as f32;
                }
            }
        }
    }

    fn engine() -> (Engine, AudioProcessor) {
        Engine::new(EngineConfig {
            max_block: 16,
            output_channels: 1,
            housekeeping_thread: false,
            ..Default::default()
        })
    }

    #[test]
    fn transport_and_sample_accurate_loop() {
        let (mut e, mut p) = engine();
        let mut d = GraphDescription::new();
        let c = d.add(Clock, &[]);
        d.set_output(c);
        e.load_graph(d).unwrap();
        e.set_loop(Some((10, 25))).unwrap();
        e.seek(20).unwrap();
        e.play().unwrap();
        let out = render_offline(&mut p, 12, 1);
        let expect: Vec<f32> = [20, 21, 22, 23, 24, 10, 11, 12, 13, 14, 15, 16]
            .iter()
            .map(|&x| x as f32)
            .collect();
        assert_eq!(out, expect);
        e.stop().unwrap();
        let out = render_offline(&mut p, 4, 1);
        assert_eq!(out, vec![0.0; 4]);
        assert_eq!(e.position(), 17);
        assert!(!e.is_playing());
    }

    #[test]
    fn graph_swap_retires_old_graph_off_thread() {
        let (mut e, mut p) = engine();
        for _ in 0..3 {
            let mut d = GraphDescription::new();
            let c = d.add(Clock, &[]);
            d.set_output(c);
            e.load_graph(d).unwrap();
        }
        render_offline(&mut p, 16, 1);
        assert_eq!(e.graphs_adopted(), 3);
        assert_eq!(e.collect_garbage(), 2);
    }

    #[test]
    fn rejects_incompatible_graph() {
        let (mut e, _p) = engine();
        let mut d = GraphDescription::new();
        let c = d.add(Clock, &[]);
        d.set_output(c);
        let g = compile(
            d,
            &CompileOptions {
                sample_rate: 44100.0,
                max_block: 16,
            },
        )
        .unwrap();
        assert!(matches!(e.publish(g), Err(EngineError::Incompatible(_))));
    }
}
