//! A read-only picture of a compiled graph for inspection and display,
//! plus optional live per-node measurements taken by the RT thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use super::compile::CompileStats;

/// One scheduled node. Indices refer to [`GraphTopology::nodes`], which is
/// in schedule (topological) order.
#[derive(Clone, Debug)]
pub struct TopologyNode {
    /// Node type, e.g. `VolumePanNode`.
    pub name: &'static str,
    /// What the node is for, e.g. `Drums · fader`, when the graph's builder
    /// gave one.
    pub label: Option<String>,
    /// What the node belongs to, as set by the graph's builder. Graphs built
    /// from an edit use the track's or bus's ID (`TrackId.0` / `BusId.0`).
    pub owner: Option<u64>,
    pub channels: usize,
    pub has_midi: bool,
    /// Latency the node itself adds.
    pub latency_samples: usize,
    /// Latency of the node's output relative to the timeline.
    pub total_latency: usize,
    /// Signal inputs.
    pub inputs: Vec<usize>,
    /// Ordering-only dependencies (e.g. automation writers).
    pub after: Vec<usize>,
}

/// The structure of a compiled graph: what actually runs, after pruning,
/// deduplication and latency compensation.
#[derive(Debug)]
pub struct GraphTopology {
    pub nodes: Vec<TopologyNode>,
    pub output: usize,
    pub stats: CompileStats,
    meters: Arc<NodeMeters>,
}

impl GraphTopology {
    pub(crate) fn new(nodes: Vec<TopologyNode>, output: usize, stats: CompileStats) -> Self {
        let meters = Arc::new(NodeMeters::new(nodes.len()));
        Self {
            nodes,
            output,
            stats,
            meters,
        }
    }

    /// Live measurements, recorded only while enabled (see
    /// [`crate::engine::Engine::set_graph_metering`]).
    pub fn meters(&self) -> &NodeMeters {
        &self.meters
    }

    pub(crate) fn meters_arc(&self) -> Arc<NodeMeters> {
        self.meters.clone()
    }
}

/// What a node did since the last [`NodeMeters::take`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NodeReading {
    /// Peak absolute sample of the node's audio output.
    pub peak: f32,
    /// Time spent in the node's `process`.
    pub busy: Duration,
}

/// Per-node output peak and processing time, written by the RT thread with
/// relaxed atomics (no allocation, no locks).
pub struct NodeMeters {
    enabled: AtomicBool,
    peak: Box<[AtomicU32]>,
    busy_ns: Box<[AtomicU64]>,
}

impl std::fmt::Debug for NodeMeters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeMeters")
            .field("enabled", &self.is_enabled())
            .finish_non_exhaustive()
    }
}

impl NodeMeters {
    fn new(len: usize) -> Self {
        Self {
            enabled: AtomicBool::new(false),
            peak: (0..len).map(|_| AtomicU32::new(0)).collect(),
            busy_ns: (0..len).map(|_| AtomicU64::new(0)).collect(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub(crate) fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    /// RT side.
    #[inline]
    pub(crate) fn record(&self, node: usize, peak: f32, busy: Duration) {
        // Non-negative floats order like their bit patterns.
        self.peak[node].fetch_max(peak.abs().to_bits(), Ordering::Relaxed);
        self.busy_ns[node].fetch_add(busy.as_nanos() as u64, Ordering::Relaxed);
    }

    /// UI side: readings per node since the previous call, then reset.
    pub fn take(&self) -> Vec<NodeReading> {
        self.peak
            .iter()
            .zip(self.busy_ns.iter())
            .map(|(p, b)| NodeReading {
                peak: f32::from_bits(p.swap(0, Ordering::Relaxed)),
                busy: Duration::from_nanos(b.swap(0, Ordering::Relaxed)),
            })
            .collect()
    }
}
