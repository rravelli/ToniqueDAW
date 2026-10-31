//! Exporting the project to audio files: the mix, and tracks on their own.

use super::ProjectState;
use crate::{
    core::export::{Export, ExportFile, ExportOptions, ExportRange, Render},
    utils::display_name,
};
use std::{path::Path, sync::Arc};
use tonique_engine::{
    edit::{BusId, ClipContent, Edit, TrackId},
    param::AtomicParam,
    time::BeatPos,
};

/// What a file of an export holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExportSource {
    /// Everything heard, through the master.
    Mix,
    /// A track or a group as heard when soloed (through its groups and the
    /// master, with what it sends to other groups), even if muted.
    Track(TrackId),
}

impl ProjectState {
    /// Start exporting `sources` into `dir`, one file each named after
    /// `name` (see [`Self::export_files`] and [`Export`]). Refused while
    /// there's nothing to export or files are still loading.
    pub fn export(
        &self,
        dir: &Path,
        name: &str,
        sources: &[ExportSource],
        options: ExportOptions,
    ) -> Result<Export, String> {
        let span = self.export_span(options.range)?;
        if sources.is_empty() {
            return Err("Choose the mix or a track to export.".into());
        }
        let mut config = self.engine_config();
        // Freed when the export ends instead.
        config.housekeeping_thread = false;
        if !self.sources_ready(config.sample_rate) {
            return Err("Some audio files are still loading. Try again in a moment.".into());
        }
        let mut edit = self.edit().clone();
        // The click is for the player only (the level is shared, not copied).
        edit.metronome = Arc::new(AtomicParam::new(0.));
        let renders = sources
            .iter()
            .zip(self.export_files(dir, name, sources))
            .map(|(source, file)| Render {
                label: file.label,
                path: file.path,
                edit: match source {
                    ExportSource::Mix => edit.detached(),
                    ExportSource::Track(id) => self.solo_edit(&edit, *id),
                },
            })
            .collect();
        Ok(Export::start(
            renders,
            config,
            span,
            options.format,
            options.tail_seconds,
        ))
    }

    /// The files exporting `sources` into `dir` writes: `<name>.wav` for
    /// the mix, `<name> - <track>.wav` for each track.
    pub fn export_files(
        &self,
        dir: &Path,
        name: &str,
        sources: &[ExportSource],
    ) -> Vec<ExportFile> {
        let mut taken: Vec<String> = Vec::new();
        sources
            .iter()
            .map(|source| {
                let (label, stem) = match source {
                    ExportSource::Mix => ("Mix".to_string(), file_safe(name, "Untitled")),
                    ExportSource::Track(id) => {
                        let track = self.track_name(*id);
                        let stem = format!(
                            "{} - {}",
                            file_safe(name, "Untitled"),
                            file_safe(&track, "Track")
                        );
                        (track, stem)
                    }
                };
                // Tracks may share a name; their files can't.
                let mut unique = stem.clone();
                let mut n = 2;
                while taken.iter().any(|t| t.eq_ignore_ascii_case(&unique)) {
                    unique = format!("{stem} {n}");
                    n += 1;
                }
                taken.push(unique.clone());
                ExportFile {
                    label,
                    path: dir.join(format!("{unique}.wav")),
                }
            })
            .collect()
    }

    /// How long an export of `range` is, in seconds, without the tail.
    pub fn export_seconds(&self, range: ExportRange) -> Result<f64, String> {
        let (start, end) = self.export_span(range)?;
        let tempo = &self.edit().tempo;
        Ok(tempo.beats_to_seconds(end) - tempo.beats_to_seconds(start))
    }

    /// The part of the timeline `range` covers, if it isn't empty.
    pub fn export_span(&self, range: ExportRange) -> Result<(BeatPos, BeatPos), String> {
        let (span, empty) = match range {
            ExportRange::Project => (
                (self.first_clip_start(), self.edit().length()),
                "The project has no clips to export.",
            ),
            ExportRange::Loop => (self.loop_range, "The loop region is empty."),
            ExportRange::Custom(start, end) => ((start, end), "The range ends before it starts."),
        };
        if span.1.0 > span.0.0 {
            Ok(span)
        } else {
            Err(empty.into())
        }
    }

    /// Where the first clip starts, or the start without clips.
    fn first_clip_start(&self) -> BeatPos {
        self.edit()
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .map(|clip| clip.start)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap_or(BeatPos::ZERO)
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

    /// The name `id` is shown with.
    fn track_name(&self, id: TrackId) -> String {
        self.all_rows()
            .into_iter()
            .find(|row| row.id == id)
            .map_or_else(String::new, |row| {
                display_name(&row.name, row.first_track_index)
            })
    }

    /// A copy of `edit` with only the track or group `id` soloed, and
    /// unmuted, apart from the project's own playback.
    fn solo_edit(&self, edit: &Edit, id: TrackId) -> Edit {
        let mut edit = edit.detached();
        for track in &mut edit.tracks {
            track.soloed = false;
        }
        for bus in &mut edit.buses {
            bus.soloed = false;
        }
        if self.is_group(id) {
            if let Ok(bus) = edit.bus_mut(BusId(id.0)) {
                bus.soloed = true;
                bus.channel.muted = false;
            }
        } else if let Ok(track) = edit.track_mut(id) {
            track.soloed = true;
            track.channel.muted = false;
        }
        edit
    }
}

/// `name` usable as a file name on any system, or `fallback` if nothing's
/// left of it.
fn file_safe(name: &str, fallback: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let safe = safe.trim().trim_end_matches('.').trim();
    if safe.is_empty() {
        fallback.into()
    } else {
        safe.into()
    }
}

#[cfg(test)]
mod tests {
    use super::file_safe;

    #[test]
    fn names_become_safe_file_names() {
        assert_eq!(file_safe("Drums / Perc", "Track"), "Drums - Perc");
        assert_eq!(file_safe("  a:b?  ", "Track"), "a-b-");
        assert_eq!(file_safe("song.", "Untitled"), "song");
        assert_eq!(file_safe("   ", "Track"), "Track");
    }
}
