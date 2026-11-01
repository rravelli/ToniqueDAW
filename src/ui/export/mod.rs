//! Export audio: what to export (the mix, tracks on their own) beside the
//! range, format and destination, with a preview of what's exported; then
//! each file drawing itself while it renders, and the result.

mod format;
mod options;
mod preview;
mod progress;
mod widgets;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use egui::{Align, Layout, Ui, vec2};
use egui_phosphor::regular::WARNING;
use tonique_engine::{edit::TrackId, time::BeatPos};

use crate::{
    core::{
        export::{Export, ExportFile, ExportOptions, ExportRange, Waveform},
        state::{ExportSource, ProjectState},
        track::TrackRow,
    },
    ui::{
        project::ProjectManager,
        taskbar::TaskbarProgress,
        theme::ThemeExt,
        widget::dialog::{self, button},
    },
};

use format::{audio_format, duration};
use options::{destination_ui, settings_ui, sources_ui};
use preview::arrangement_preview;
use progress::{done_ui, failed_ui, running_ui};
use widgets::{header, note};

/// The options' width: what to export on the left, how on the right.
const WIDTH: f32 = 720.;
const SOURCES_WIDTH: f32 = 250.;
const COLUMN_GAP: f32 = 20.;
const CHOICE_HEIGHT: f32 = 26.;
/// Space around the preview's lanes, and a rendered file's waveform.
const LANE_PADDING: f32 = 6.;

enum Step {
    Closed,
    /// Choosing what to export and where.
    Options,
    Running(Export),
    Done {
        files: Vec<ExportFile>,
        waveforms: Vec<Waveform>,
        seconds: f64,
    },
    Failed {
        title: String,
        detail: String,
    },
}

/// What's chosen to export, kept between exports.
struct Selection {
    mix: bool,
    tracks: HashSet<TrackId>,
}

impl Selection {
    /// The files to write, in the track list's order, the mix first.
    fn sources(&self, state: &ProjectState) -> Vec<ExportSource> {
        let tracks = state
            .all_rows()
            .into_iter()
            .filter(|track| self.tracks.contains(&track.id))
            .map(|track| ExportSource::Track(track.id));
        self.mix
            .then_some(ExportSource::Mix)
            .into_iter()
            .chain(tracks)
            .collect()
    }

    /// Whether `track` is heard in what's exported: in the mix, or chosen
    /// with or inside a chosen group.
    fn includes(&self, state: &ProjectState, track: &TrackRow) -> bool {
        if self.mix {
            return !track.is_silenced();
        }
        let mut id = Some(track.id);
        while let Some(current) = id {
            if self.tracks.contains(&current) {
                return true;
            }
            id = state.parent(current);
        }
        false
    }
}

pub struct ExportWindow {
    step: Step,
    /// Kept between exports.
    options: ExportOptions,
    selection: Selection,
    /// Where the files go: the project's folder unless another was chosen.
    dir: String,
    /// What the files are named after: the project, at first.
    name: String,
    /// The custom range, kept while another is chosen; the whole project
    /// at first (`None` until the options are first shown).
    custom: Option<(BeatPos, BeatPos)>,
    /// Which of the files to write already exist, checked when the files
    /// change and each time the window opens rather than every frame.
    existing: Option<(Vec<PathBuf>, HashSet<PathBuf>)>,
    taskbar: TaskbarProgress,
}

impl ExportWindow {
    pub fn new() -> Self {
        Self {
            step: Step::Closed,
            options: ExportOptions::default(),
            selection: Selection {
                mix: true,
                tracks: HashSet::new(),
            },
            dir: String::new(),
            name: String::new(),
            custom: None,
            existing: None,
            taskbar: TaskbarProgress::new(),
        }
    }

    /// Ask what to export. Ignored while an export runs.
    pub fn open(&mut self, project: &ProjectManager) {
        if matches!(self.step, Step::Running(_)) {
            return;
        }
        self.name = project.name();
        if let Some(dir) = project.dir() {
            self.dir = dir.display().to_string();
        }
        self.existing = None;
        self.step = Step::Options;
    }

    pub fn show(&mut self, ui: &Ui, state: &ProjectState) {
        let next = match &mut self.step {
            Step::Closed => None,
            Step::Options => self.options_ui(ui, state),
            Step::Running(export) => running_ui(ui, export),
            Step::Done {
                files,
                waveforms,
                seconds,
            } => done_ui(ui, files, waveforms, *seconds),
            Step::Failed { title, detail } => failed_ui(ui, title, detail),
        };
        if let Some(next) = next {
            self.step = next;
        }
        self.taskbar.set(match &self.step {
            Step::Running(export) => Some(export.progress()),
            _ => None,
        });
    }

    fn options_ui(&mut self, ui: &Ui, state: &ProjectState) -> Option<Step> {
        let theme = ui.app_theme();
        // Tracks deleted since are no longer chosen.
        let rows = state.all_rows();
        self.selection
            .tracks
            .retain(|id| rows.iter().any(|row| row.id == *id));
        // Seeded once only: an end typed before the start stays, with the
        // problem shown, until the start or end is corrected.
        let custom = self.custom.get_or_insert_with(|| {
            state.export_span(ExportRange::Project).unwrap_or((
                BeatPos::ZERO,
                BeatPos(state.time_signature().beats_per_bar() * 4.),
            ))
        });
        let sources = self.selection.sources(state);
        let dir = PathBuf::from(self.dir.trim());
        let files = state.export_files(&dir, &self.name, &sources);
        let paths: Vec<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
        let existing = match &mut self.existing {
            Some((checked, existing)) if *checked == paths => &*existing,
            slot => {
                let existing = paths.iter().filter(|p| p.exists()).cloned().collect();
                &slot.insert((paths, existing)).1
            }
        };
        let problem = state
            .export_span(self.options.range)
            .err()
            .or_else(|| {
                sources
                    .is_empty()
                    .then(|| "Choose the mix or a track to export.".into())
            })
            .or_else(|| {
                if self.dir.trim().is_empty() {
                    Some("Choose a folder to export into.".into())
                } else {
                    (!dir.is_dir()).then(|| "The folder doesn't exist.".into())
                }
            });

        let mut next = None;
        let modal = dialog::modal(ui, "export-options").show(ui.ctx(), |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing = vec2(6., 8.);
            let subtitle = match state.export_seconds(self.options.range) {
                Ok(seconds) => format!(
                    "{}  ·  {}  ·  {}",
                    self.name,
                    duration(seconds + self.options.tail_seconds as f64),
                    audio_format(state)
                ),
                Err(_) => self.name.clone(),
            };
            header(ui, "Export audio", &subtitle);
            if let Some(dragged) =
                arrangement_preview(ui, state, &rows, &self.selection, self.options.range)
            {
                *custom = dragged;
                self.options.range = ExportRange::Custom(dragged.0, dragged.1);
            }
            ui.add_space(2.);

            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = COLUMN_GAP;
                ui.vertical(|ui| {
                    ui.set_width(SOURCES_WIDTH);
                    ui.spacing_mut().item_spacing.y = 8.;
                    sources_ui(ui, &rows, &mut self.selection);
                });
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 8.;
                    settings_ui(ui, state, &mut self.options, custom, files.len());
                    destination_ui(ui, &mut self.dir, &mut self.name, &files, existing);
                });
            });

            ui.add_space(4.);
            ui.horizontal(|ui| {
                if let Some(problem) = &problem {
                    note(ui, WARNING, theme.danger, problem);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = match files.len() {
                        0 | 1 => "Export".to_string(),
                        n => format!("Export {n} files"),
                    };
                    let export = ui.add_enabled(problem.is_none(), button(&label).selected(true));
                    if export.clicked() {
                        next = Some(start(state, &dir, &self.name, &sources, self.options));
                    }
                    if ui.add(button("Cancel")).clicked() {
                        next = Some(Step::Closed);
                    }
                });
            });
        });
        if next.is_none() && modal.should_close() {
            next = Some(Step::Closed);
        }
        next
    }
}

fn start(
    state: &ProjectState,
    dir: &Path,
    name: &str,
    sources: &[ExportSource],
    options: ExportOptions,
) -> Step {
    match state.export(dir, name, sources, options) {
        Ok(export) => Step::Running(export),
        Err(detail) => Step::Failed {
            title: "Couldn't export".into(),
            detail,
        },
    }
}
