use std::path::PathBuf;

use crate::core::{export::ExportStatus, message::GuiToPlayerMsg, state::ToniqueProjectState};

impl ToniqueProjectState {
    /// Get current status for the export
    pub fn export_status(&self) -> &ExportStatus {
        &self.export_status
    }
    /// Export the project at location
    pub fn export(&mut self, path: PathBuf) {
        if self.tx.push(GuiToPlayerMsg::Export(path)).is_ok() {
            self.export_status = ExportStatus::PROCESSING(0.);
        };
    }
}
