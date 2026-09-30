//! Export audio: the options, the save dialog, progress and the result.

use std::path::PathBuf;

use egui::{Id, Modal, ProgressBar, RichText, Ui, vec2};
use rfd::FileDialog;
use tonique_engine::sample::WavFormat;

use crate::{
    core::{
        export::{Export, ExportOptions, ExportOutcome, ExportRange},
        project::project_name,
        state::ProjectState,
    },
    ui::{
        project::ProjectManager,
        theme::ThemeExt,
        widget::{dialog::button, select::Select},
    },
};

enum Step {
    Closed,
    /// Choosing what to export; `file_name` and `dir` suggest where.
    Options {
        file_name: String,
        dir: Option<PathBuf>,
    },
    Running(Export),
    Finished {
        title: String,
        detail: String,
    },
}

pub struct ExportWindow {
    step: Step,
    /// Kept between exports.
    options: ExportOptions,
}

impl ExportWindow {
    pub fn new() -> Self {
        Self {
            step: Step::Closed,
            options: ExportOptions::default(),
        }
    }

    /// Ask what to export. Ignored while an export runs.
    pub fn open(&mut self, project: &ProjectManager) {
        if matches!(self.step, Step::Running(_)) {
            return;
        }
        self.step = Step::Options {
            file_name: format!("{}.wav", project.name()),
            dir: project.dir().map(PathBuf::from),
        };
    }

    pub fn show(&mut self, ui: &Ui, state: &ProjectState) {
        let next = match &mut self.step {
            Step::Closed => None,
            Step::Options { file_name, dir } => {
                options_ui(ui, state, &mut self.options, file_name, dir.as_deref())
            }
            Step::Running(export) => running_ui(ui, export),
            Step::Finished { title, detail } => finished_ui(ui, title, detail),
        };
        if let Some(next) = next {
            self.step = next;
        }
    }
}

fn options_ui(
    ui: &Ui,
    state: &ProjectState,
    options: &mut ExportOptions,
    file_name: &str,
    dir: Option<&std::path::Path>,
) -> Option<Step> {
    let theme = ui.app_theme();
    let mut next = None;
    let modal = Modal::new(Id::new("export-options")).show(ui.ctx(), |ui| {
        ui.set_width(320.);
        ui.spacing_mut().item_spacing = vec2(6., 8.);
        ui.label(RichText::new("Export audio").strong());

        for (range, label) in [
            (ExportRange::Project, "Whole project"),
            (ExportRange::Loop, "Loop region"),
        ] {
            let length = match state.export_seconds(range) {
                Ok(seconds) => format!("{label} ({})", duration(seconds)),
                Err(_) => format!("{label} (empty)"),
            };
            ui.radio_value(&mut options.range, range, length);
        }
        ui.horizontal(|ui| {
            ui.label("Format");
            ui.add(
                Select::new("export-format", &mut options.format)
                    .option(WavFormat::Int16, "WAV 16-bit")
                    .option(WavFormat::Int24, "WAV 24-bit")
                    .option(WavFormat::Float32, "WAV 32-bit float")
                    .width(140.),
            );
        });
        let rate = state.engine_config().sample_rate;
        ui.label(
            RichText::new(format!(
                "{:.1} kHz, the audio output's sample rate.",
                rate / 1000.
            ))
            .color(theme.text_muted)
            .small(),
        );
        let problem = state.export_span(options.range).err();
        if let Some(problem) = &problem {
            ui.label(RichText::new(problem).color(theme.danger).small());
        }

        ui.horizontal(|ui| {
            let export = ui.add_enabled(problem.is_none(), button("Export…").selected(true));
            if export.clicked() {
                next = Some(start(state, *options, file_name, dir));
            }
            if ui.add(button("Cancel")).clicked() {
                next = Some(Step::Closed);
            }
        });
    });
    if next.is_none() && modal.should_close() {
        next = Some(Step::Closed);
    }
    next
}

/// Ask where to save, then start. Back to the options if no file is chosen.
fn start(
    state: &ProjectState,
    options: ExportOptions,
    file_name: &str,
    dir: Option<&std::path::Path>,
) -> Step {
    let mut dialog = FileDialog::new()
        .add_filter("WAV audio", &["wav"])
        .set_file_name(file_name);
    if let Some(dir) = dir {
        dialog = dialog.set_directory(dir);
    }
    let Some(mut path) = dialog.save_file() else {
        return Step::Options {
            file_name: file_name.to_string(),
            dir: dir.map(PathBuf::from),
        };
    };
    if path.extension().is_none_or(|ext| ext != "wav") {
        path.set_extension("wav");
    }
    match state.export(path, options) {
        Ok(export) => Step::Running(export),
        Err(detail) => Step::Finished {
            title: "Couldn't export".into(),
            detail,
        },
    }
}

fn running_ui(ui: &Ui, export: &mut Export) -> Option<Step> {
    let name = project_name(export.path());
    if let Some(outcome) = export.outcome() {
        return Some(match outcome {
            ExportOutcome::Done => Step::Finished {
                title: format!("Exported “{name}”"),
                detail: export.path().display().to_string(),
            },
            ExportOutcome::Cancelled => Step::Closed,
            ExportOutcome::Failed(e) => Step::Finished {
                title: format!("Couldn't export “{name}”"),
                detail: e.clone(),
            },
        });
    }
    Modal::new(Id::new("export-progress")).show(ui.ctx(), |ui| {
        ui.set_width(320.);
        ui.spacing_mut().item_spacing = vec2(6., 8.);
        ui.label(RichText::new(format!("Exporting “{name}”…")).strong());
        ui.add(ProgressBar::new(export.progress()).show_percentage());
        if ui.add(button("Cancel")).clicked() {
            export.cancel();
        }
    });
    None
}

fn finished_ui(ui: &Ui, title: &str, detail: &str) -> Option<Step> {
    let theme = ui.app_theme();
    let mut close = false;
    let modal = Modal::new(Id::new("export-finished")).show(ui.ctx(), |ui| {
        ui.set_width(360.);
        ui.spacing_mut().item_spacing = vec2(6., 6.);
        ui.label(RichText::new(title).strong());
        ui.label(RichText::new(detail).color(theme.text_muted));
        close = ui.add(button("OK").selected(true)).clicked();
    });
    (close || modal.should_close()).then_some(Step::Closed)
}

/// `m:ss`, or `m:ss.s` under ten seconds.
fn duration(seconds: f64) -> String {
    let minutes = (seconds / 60.).floor();
    let rest = seconds - minutes * 60.;
    if seconds < 10. {
        format!("{minutes:.0}:{rest:04.1}")
    } else {
        format!("{minutes:.0}:{:02.0}", rest.floor())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_as_minutes_and_seconds() {
        assert_eq!(duration(2.5), "0:02.5");
        assert_eq!(duration(75.9), "1:15");
        assert_eq!(duration(600.), "10:00");
    }
}
