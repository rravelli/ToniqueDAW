//! Parallel, lock-free execution of a [`CompiledGraph`].
//!
//! The graph already carries per-node dependency counts and dependents, so a
//! block is just "drain the DAG": seed the ready queue with roots, and every
//! participant pops a node, runs it, and decrements its dependents' counters,
//! pushing any that reach zero. The audio callback thread participates too,
//! so there's no hand-off latency, and nothing on this path takes a lock.

use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle, Thread};
use std::time::Duration;

use super::compiled::CompiledGraph;
use super::node::BlockInfo;

struct Job {
    graph: *const CompiledGraph,
    info: BlockInfo,
}

struct Shared {
    /// Non-null only while the audio thread is inside `run_block`.
    job: AtomicPtr<Job>,
    epoch: AtomicU64,
    /// Workers currently holding a reference to `job`.
    active: AtomicUsize,
    shutdown: AtomicBool,
}

/// Helper threads that assist the audio thread with graph processing.
pub struct WorkerPool {
    shared: Arc<Shared>,
    threads: Vec<Thread>,
    handles: Vec<JoinHandle<()>>,
}

const SPIN_ITERATIONS: u32 = 4096;

impl WorkerPool {
    /// `helpers` threads in addition to the audio thread. Helpers run at
    /// normal priority: if one is starved it simply doesn't pick up work.
    pub fn new(helpers: usize) -> Self {
        let shared = Arc::new(Shared {
            job: AtomicPtr::new(ptr::null_mut()),
            epoch: AtomicU64::new(0),
            active: AtomicUsize::new(0),
            shutdown: AtomicBool::new(false),
        });
        let handles: Vec<JoinHandle<()>> = (0..helpers)
            .map(|i| {
                let shared = shared.clone();
                thread::Builder::new()
                    .name(format!("tonique-worker-{i}"))
                    .spawn(move || worker_main(&shared))
                    .expect("spawn worker")
            })
            .collect();
        let threads = handles.iter().map(|h| h.thread().clone()).collect();
        Self {
            shared,
            threads,
            handles,
        }
    }

    pub fn helpers(&self) -> usize {
        self.threads.len()
    }

    /// Process one block of `graph` using this pool plus the calling thread.
    /// Returns once every node has run and no helper still references it.
    pub fn run_block(&self, graph: &mut CompiledGraph, info: &BlockInfo) {
        assert!(info.block_len <= graph.max_block());
        let graph: &CompiledGraph = graph;
        graph.reset_counters();
        for &r in graph.roots() {
            let _ = graph.ready.push(r);
        }
        let mut job = Job { graph, info: *info };
        self.shared.job.store(&mut job, Ordering::SeqCst);
        self.shared.epoch.fetch_add(1, Ordering::SeqCst);
        for t in &self.threads {
            t.unpark();
        }

        drain(graph, info);

        // Retract the job, then wait for helpers that may still hold it. A
        // helper increments `active` *before* loading `job`, so (SeqCst)
        // either it sees null, or we see it as active here.
        self.shared.job.store(ptr::null_mut(), Ordering::SeqCst);
        while self.shared.active.load(Ordering::SeqCst) != 0 {
            std::hint::spin_loop();
        }
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        for t in &self.threads {
            t.unpark();
        }
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

/// Pull ready nodes until the whole graph has been processed.
fn drain(graph: &CompiledGraph, info: &BlockInfo) {
    let total = graph.len();
    while graph.done.load(Ordering::Acquire) < total {
        match graph.ready.pop() {
            Some(idx) => {
                let idx = idx as usize;
                // SAFETY: a node enters the ready queue exactly once per block,
                // after all its predecessors decremented its counter (AcqRel),
                // so it runs on one thread and sees their output.
                unsafe { graph.process_node(idx, info) };
                for &d in graph.dependents(idx) {
                    if graph.remaining[d as usize].fetch_sub(1, Ordering::AcqRel) == 1 {
                        let _ = graph.ready.push(d);
                    }
                }
                graph.done.fetch_add(1, Ordering::Release);
            }
            None => std::hint::spin_loop(),
        }
    }
}

fn worker_main(shared: &Shared) {
    crate::rt::enable_flush_denormals();
    let mut seen_epoch = 0;
    let mut idle_spins = 0u32;
    loop {
        if shared.shutdown.load(Ordering::Acquire) {
            return;
        }
        let epoch = shared.epoch.load(Ordering::Acquire);
        if epoch != seen_epoch {
            seen_epoch = epoch;
            idle_spins = 0;
            shared.active.fetch_add(1, Ordering::SeqCst);
            let job = shared.job.load(Ordering::SeqCst);
            if !job.is_null() {
                // SAFETY: the audio thread keeps `job` (and its graph) alive
                // until `active` drops to zero.
                let job = unsafe { &*job };
                crate::rt::no_alloc(|| drain(unsafe { &*job.graph }, &job.info));
            }
            shared.active.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        // Bounded spin (the next block is usually only a few ms away), then park.
        if idle_spins < SPIN_ITERATIONS {
            idle_spins += 1;
            std::hint::spin_loop();
        } else {
            thread::park_timeout(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{
        CompileOptions, GraphDescription, Node, NodeProperties, ProcessContext, compile,
    };
    use crate::nodes::SumNode;

    struct Ramp(f32);
    impl Node for Ramp {
        fn properties(&self) -> NodeProperties {
            NodeProperties::audio(2)
        }
        fn process(&mut self, ctx: &mut ProcessContext) {
            for ch in 0..2 {
                for (i, s) in ctx.audio_out.channel_mut(ch).iter_mut().enumerate() {
                    *s = self.0 + i as f32;
                }
            }
        }
    }

    fn wide_graph() -> CompiledGraph {
        let mut d = GraphDescription::new();
        let mut tops = Vec::new();
        for t in 0..32 {
            let mut prev = d.add(Ramp(t as f32), &[]);
            for _ in 0..4 {
                prev = d.add(SumNode::new(2), &[prev]);
            }
            tops.push(prev);
        }
        let out = d.add(SumNode::new(2), &tops);
        d.set_output(out);
        compile(
            d,
            &CompileOptions {
                sample_rate: 48000.0,
                max_block: 128,
            },
        )
        .unwrap()
    }

    #[test]
    fn parallel_matches_sequential() {
        let info = BlockInfo {
            block_len: 128,
            sample_rate: 48000.0,
            timeline_pos: 0,
            playing: true,
            jumped: false,
        };
        let mut seq = wide_graph();
        seq.process_sequential(&info);
        let expected = seq.output(128).channel(1).to_vec();

        let pool = WorkerPool::new(3);
        let mut par = wide_graph();
        for _ in 0..200 {
            pool.run_block(&mut par, &info);
            assert_eq!(par.output(128).channel(1), &expected[..]);
        }
    }
}
