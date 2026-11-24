//! The project's side of the monitor (`crate::monitor`): reading the
//! meters and the graph view's measures, forgetting a meter's clips.

use tonique_engine::edit::TrackId;

use super::{MASTER_TRACK_ID, ProjectState};
use crate::{monitor::graph_monitor::GraphMonitor, monitor::metrics::GlobalMetrics};

impl ProjectState {
    /// The channels' meters and the processor's load.
    pub fn metrics(&self) -> &GlobalMetrics {
        &self.monitor.metrics
    }

    /// The engine's graph and its measures, for the graph view.
    pub fn graph(&self) -> &GraphMonitor {
        &self.monitor.graph
    }

    /// Measure the engine's graph (for the graph view) from the next update.
    pub fn set_monitor_graph(&mut self, on: bool) {
        self.monitor.monitoring = on;
    }

    /// Forget the clips (and held peaks) a track's meter shows.
    pub fn reset_meter_clip(&mut self, id: TrackId) {
        let metrics = &mut self.monitor.metrics;
        if id == MASTER_TRACK_ID {
            metrics.master.reset_clip();
        }
        if let Some(track) = metrics.tracks.get_mut(&id) {
            track.reset_clip();
        }
    }
}
