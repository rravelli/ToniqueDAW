//! Exporting the project to an audio file.

use super::ProjectState;
use crate::core::export::{Export, ExportOptions, ExportRange};
use std::{path::PathBuf, sync::Arc};
use tonique_engine::{edit::ClipContent, param::AtomicParam, time::BeatPos};

impl ProjectState {
    /// Start exporting to `path` (see [`Export`]). Refused while there's
    /// nothing to export or files are still loading.
    pub fn export(&self, path: PathBuf, options: ExportOptions) -> Result<Export, String> {
        let span = self.export_span(options.range)?;
        let mut config = self.engine_config();
        // Freed when the export ends instead.
        config.housekeeping_thread = false;
        if !self.sources_ready(config.sample_rate) {
            return Err("Some audio files are still loading. Try again in a moment.".into());
        }
        let mut edit = self.edit().clone();
        // The click is for the player only (the level is shared, not copied).
        edit.metronome = Arc::new(AtomicParam::new(0.));
        Ok(Export::start(edit, config, span, path, options.format))
    }

    /// How long an export of `range` is, in seconds.
    pub fn export_seconds(&self, range: ExportRange) -> Result<f64, String> {
        let (start, end) = self.export_span(range)?;
        let tempo = &self.edit().tempo;
        Ok(tempo.beats_to_seconds(end) - tempo.beats_to_seconds(start))
    }

    /// The part of the timeline `range` covers, if it isn't empty.
    pub fn export_span(&self, range: ExportRange) -> Result<(BeatPos, BeatPos), String> {
        let (span, empty) = match range {
            ExportRange::Project => (
                (BeatPos::ZERO, self.edit().length()),
                "The project has no clips to export.",
            ),
            ExportRange::Loop => (self.loop_range, "The loop region is empty."),
        };
        if span.1.0 > span.0.0 {
            Ok(span)
        } else {
            Err(empty.into())
        }
    }

    /// Whether every clip's audio is decoded and at the engine's rate.
    pub(super) fn sources_ready(&self, rate: f64) -> bool {
        let edit = self.edit();
        edit.tracks
            .iter()
            .flat_map(|track| &track.clips)
            .all(|clip| match clip.content {
                ClipContent::Audio { source, .. } => edit
                    .source(source)
                    .is_some_and(|data| data.sample_rate() == rate),
                ClipContent::Midi { .. } => true,
            })
    }
}
