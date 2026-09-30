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
//! ```
//!
//! The processor never frees memory: everything it lets go of goes back
//! through the garbage ring and is dropped by a housekeeping thread.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::graph::scheduler::WorkerPool;
use crate::graph::{
    BlockInfo, CompileError, CompileOptions, CompileStats, CompiledGraph, GraphDescription,
    GraphTopology, NodeIdentity, NodeMessage, compile,
};
use crate::preview::{PreviewControl, PreviewShared, PreviewSource, PreviewStream};
use crate::rt;
use crate::time::SamplePos;

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
    /// Loop `[start, end)` while playing; `None` disables looping.
    SetLoop(Option<(SamplePos, SamplePos)>),
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
}

/// Things the RT thread has let go of, to be dropped elsewhere.
pub enum Garbage {
    Graph(Box<CompiledGraph>),
    Message(NodeMessage),
    Command(Command),
}

#[derive(Debug)]
pub enum EngineError {
    Compile(CompileError),
    CommandQueueFull,
    Incompatible(&'static str),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compile(e) => write!(f, "compile error: {e}"),
            Self::CommandQueueFull => write!(f, "command queue full"),
            Self::Incompatible(why) => write!(f, "incompatible graph: {why}"),
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
    playing: AtomicBool,
    graphs_adopted: AtomicU64,
    blocks: AtomicU64,
    /// Nodes whose state was carried over by the last adoption.
    last_migrated: AtomicU64,
    /// Time spent in the last callback relative to its real-time budget, in 1/1000.
    load_permille: AtomicU64,
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
    jumped: bool,
    preview: Option<PreviewStream>,
    preview_generation: u64,
    preview_shared: Arc<PreviewShared>,
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
            jumped: true,
            preview: None,
            preview_generation: 0,
            preview_shared: preview.shared(),
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

    pub fn set_loop(&mut self, range: Option<(SamplePos, SamplePos)>) -> Result<(), EngineError> {
        self.send(Command::SetLoop(range))
    }

    pub fn position(&self) -> SamplePos {
        self.shared.position.load(Ordering::Relaxed)
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

    /// True from `preview_play`/`preview_seek` until stopped or played to the end.
    pub fn is_previewing(&self) -> bool {
        self.preview.is_playing()
    }

    /// Preview position in frames of the source, while previewing.
    pub fn preview_position(&self) -> Option<usize> {
        self.preview.position()
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

    /// Render one device buffer. `out` is interleaved with `channels`
    /// channels. Allocation-free, lock-free and bounded.
    pub fn process_interleaved(&mut self, out: &mut [f32], channels: usize) {
        let started = Instant::now();
        rt::no_alloc(|| {
            self.handle_commands();
            self.adopt_graphs();
            let frames = out.len() / channels;
            let mut done = 0;
            while done < frames {
                let mut n = (frames - done).min(self.config.max_block);
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
                done += n;
            }
            self.mix_preview(out, channels);
            self.shared.position.store(self.position, Ordering::Relaxed);
            self.shared.playing.store(self.playing, Ordering::Relaxed);
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

    /// Add queued preview frames to the first two channels of `out`.
    fn mix_preview(&mut self, out: &mut [f32], channels: usize) {
        let Some(stream) = &mut self.preview else {
            return;
        };
        let frames = out.len() / channels.max(1);
        let n = stream.slots().min(frames);
        if let Ok(chunk) = stream.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            for (frame, [l, r]) in out.chunks_exact_mut(channels).zip(a.iter().chain(b)) {
                frame[0] += l;
                if channels > 1 {
                    frame[1] += r;
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
        let info = BlockInfo {
            block_len: n,
            sample_rate: self.config.sample_rate,
            timeline_pos: self.position,
            playing: self.playing,
            jumped: self.jumped,
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
                    if !self.playing {
                        self.playing = true;
                        self.jumped = true;
                    }
                }
                Command::Stop => self.playing = false,
                Command::Seek(pos) => {
                    self.position = pos;
                    self.jumped = true;
                }
                Command::SetLoop(range) => self.loop_range = range.filter(|(s, e)| e > s),
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
