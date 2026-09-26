use std::{sync::Arc, time::Instant};
use tonique_engine::{engine::Engine, graph::GraphTopology};

/// Fraction of the level kept per UI frame: peaks fall back smoothly.
const LEVEL_DECAY: f32 = 0.85;
/// Weight of the newest CPU reading.
const CPU_SMOOTHING: f32 = 0.2;

/// The engine's current graph plus smoothed live readings per node, for the
/// graph view. Only measures while enabled, since measuring costs a little
/// time on the audio thread.
pub struct GraphMonitor {
    enabled: bool,
    topology: Option<Arc<GraphTopology>>,
    /// Output peak per node, with a falling decay.
    pub levels: Vec<f32>,
    /// Share of one core spent in each node.
    pub cpu: Vec<f32>,
    last_read: Instant,
}

impl GraphMonitor {
    pub fn new() -> Self {
        Self {
            enabled: false,
            topology: None,
            levels: Vec::new(),
            cpu: Vec::new(),
            last_read: Instant::now(),
        }
    }

    /// Forget everything, e.g. after the engine was replaced.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn topology(&self) -> Option<&Arc<GraphTopology>> {
        self.topology.as_ref()
    }

    /// Call once per frame.
    pub fn update(&mut self, engine: &mut Engine, enabled: bool) {
        if enabled != self.enabled {
            engine.set_graph_metering(enabled);
            self.enabled = enabled;
        }
        if !enabled {
            return;
        }
        let topology = engine.graph_topology();
        let changed = match (&topology, &self.topology) {
            (Some(new), Some(old)) => !Arc::ptr_eq(new, old),
            (new, old) => new.is_some() != old.is_some(),
        };
        if changed {
            let len = topology.as_ref().map_or(0, |t| t.nodes.len());
            self.levels = vec![0.; len];
            self.cpu = vec![0.; len];
            self.topology = topology;
        }
        let Some(topology) = &self.topology else {
            return;
        };
        let elapsed = self.last_read.elapsed().as_secs_f32().max(1e-6);
        self.last_read = Instant::now();
        for (i, reading) in topology.meters().take().into_iter().enumerate() {
            self.levels[i] = reading.peak.max(self.levels[i] * LEVEL_DECAY);
            let share = reading.busy.as_secs_f32() / elapsed;
            self.cpu[i] += CPU_SMOOTHING * (share - self.cpu[i]);
        }
    }
}
