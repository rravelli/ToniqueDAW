//! What the UI shows of the engine at work: the channels' meters, the
//! processor's load, and the graph view's measures.

use tonique_engine::edit::{EditSession, TrackId};

use super::{MASTER_TRACK_ID, ProjectState};
use crate::{diagnostics::graph_monitor::GraphMonitor, diagnostics::metrics::GlobalMetrics};

#[derive(Default)]
pub(super) struct Diagnostics {
    pub(super) metrics: GlobalMetrics,
    /// Live graph data for the graph view (measured only while it's shown).
    pub(super) graph: GraphMonitor,
    /// Whether the graph monitor measures: only while the graph is shown.
    pub(super) monitoring: bool,
}

impl Diagnostics {
    /// Reads the meters and the load, and the graph while it's shown.
    pub(super) fn update(&mut self, session: &mut EditSession) {
        let edit = session.edit();
        self.metrics.master.update(edit.master.meter(), true);
        self.metrics
            .tracks
            .insert(MASTER_TRACK_ID, self.metrics.master.clone());
        let channels = edit
            .tracks
            .iter()
            .map(|track| (track.id, &track.channel))
            .chain(
                edit.buses
                    .iter()
                    .map(|bus| (TrackId(bus.id.0), &bus.channel)),
            );
        for (id, channel) in channels {
            self.metrics
                .tracks
                .entry(id)
                .or_default()
                .update(channel.meter(), false);
        }
        self.metrics.latency = session.engine().cpu_load();
        self.graph.update(session.engine_mut(), self.monitoring);
    }
}

impl ProjectState {
    /// The channels' meters and the processor's load.
    pub fn metrics(&self) -> &GlobalMetrics {
        &self.diagnostics.metrics
    }

    /// The engine's graph and its measures, for the graph view.
    pub fn graph(&self) -> &GraphMonitor {
        &self.diagnostics.graph
    }

    /// Measure the engine's graph (for the graph view) from the next update.
    pub fn set_monitor_graph(&mut self, on: bool) {
        self.diagnostics.monitoring = on;
    }

    /// Forget the clips (and held peaks) a track's meter shows.
    pub fn reset_meter_clip(&mut self, id: TrackId) {
        let metrics = &mut self.diagnostics.metrics;
        if id == MASTER_TRACK_ID {
            metrics.master.reset_clip();
        }
        if let Some(track) = metrics.tracks.get_mut(&id) {
            track.reset_clip();
        }
    }
}
