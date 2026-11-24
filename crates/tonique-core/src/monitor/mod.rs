//! What the UI shows of the engine at work: the channels' meters, the
//! processor's load, the audio graph's live measures, and where the graph
//! view puts its nodes.

pub mod graph_layout;
pub mod graph_monitor;
pub mod metrics;

use tonique_engine::edit::{EditSession, TrackId};

use crate::document::track::MASTER_TRACK_ID;
use graph_monitor::GraphMonitor;
use metrics::GlobalMetrics;

#[derive(Default)]
pub(crate) struct Monitor {
    pub(crate) metrics: GlobalMetrics,
    /// Live graph data for the graph view (measured only while it's shown).
    pub(crate) graph: GraphMonitor,
    /// Whether the graph monitor measures: only while the graph is shown.
    pub(crate) monitoring: bool,
}

impl Monitor {
    /// Reads the meters and the load, and the graph while it's shown.
    pub(crate) fn update(&mut self, session: &mut EditSession) {
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
