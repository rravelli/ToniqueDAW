//! Export audio, as the egui app's export window: what to export (the mix,
//! tracks on their own) beside the range, format and destination, with a
//! preview of what's exported; then each file drawing itself while it
//! renders, and the result.
//!
//! Unlike the egui app, the custom range's bound fields are typed rather
//! than dragged, dragging in the preview moves one bound (the nearest)
//! rather than drawing a new range, and the taskbar shows no progress.

mod options;
mod parts;
mod preview;
mod progress;

use crate::model::HoldsProject;
use std::{cell::Cell, collections::HashSet, path::PathBuf, rc::Rc};

use gpui::{App, Bounds, Context, MouseDownEvent, MouseMoveEvent, Pixels, Window};
use tonique_engine::time::BeatPos;

use crate::{
    app::{Tonique, dialog::Dialog},
    core::{
        export::{Export, ExportFile, ExportOptions, ExportOutcome, ExportRange, Waveform},
        state::ExportSelection,
    },
    widgets::TextField,
};

/// Room for a rendered file's waveform, and around the preview's lanes.
const LANE_PADDING: f32 = 6.;
const CHOICE_HEIGHT: f32 = 24.;

enum Step {
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

/// Which bound of the custom range.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Bound {
    Start,
    End,
}

pub struct ExportUi {
    step: Step,
    /// Kept between exports, as what's chosen.
    options: ExportOptions,
    selection: ExportSelection,
    /// Where the files go: the project's folder unless another was chosen.
    folder: TextField,
    /// What the files are named after: the project, at first.
    name: TextField,
    /// The custom range, kept while another is chosen; the whole project
    /// at first (`None` until the options are first shown).
    custom: Option<(BeatPos, BeatPos)>,
    /// A bound of the custom range being typed.
    typing: Option<(Bound, TextField)>,
    /// Which of the files to write already exist, checked when the files
    /// change and each time the dialog opens rather than every frame.
    existing: Option<(Vec<PathBuf>, HashSet<PathBuf>)>,
    /// The preview, as last laid out, for dragging across it.
    preview_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// While dragging in the preview: the bound it moves.
    dragged_bound: Option<Bound>,
}

impl ExportUi {
    pub fn new(context: &mut Context<Tonique>) -> Self {
        Self {
            step: Step::Options,
            options: ExportOptions::default(),
            selection: ExportSelection::default(),
            folder: TextField::new(context),
            name: TextField::new(context),
            custom: None,
            typing: None,
            existing: None,
            preview_bounds: Rc::default(),
            dragged_bound: None,
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self.step, Step::Running(_))
    }

    pub fn is_dragging(&self) -> bool {
        self.dragged_bound.is_some()
    }

    /// Sets the custom range, and exports it if it's the range chosen.
    fn set_custom(&mut self, range: (BeatPos, BeatPos)) {
        self.custom = Some(range);
        if matches!(self.options.range, ExportRange::Custom(..)) {
            self.options.range = ExportRange::Custom(range.0, range.1);
        }
    }
}

impl Tonique {
    /// Asks what to export. Ignored while an export runs.
    pub fn open_export(&mut self, window: &mut Window, context: &mut Context<Self>) {
        let export = &mut self.export;
        if export.is_running() {
            return;
        }
        export.name.text = self.files.name();
        if let Some(dir) = self.files.dir() {
            export.folder.text = dir.display().to_string();
        }
        export.existing = None;
        export.typing = None;
        export.step = Step::Options;
        self.popup = None;
        self.dialog = Some(Dialog::Export);
        window.focus(&self.focus, context);
    }

    /// Whether the export dialog may close: not while files render.
    pub(super) fn export_closes(&self) -> bool {
        !self.export.is_running()
    }

    fn close_export(&mut self, window: &mut Window, context: &mut Context<Self>) {
        self.dialog = None;
        self.export.typing = None;
        window.focus(&self.focus, context);
    }

    fn start_export(&mut self, window: &mut Window, context: &mut Context<Self>) {
        let export = &mut self.export;
        let sources = export.selection.sources(&self.project.read(context).state);
        let dir = PathBuf::from(export.folder.text.trim());
        let started = self.project.read(context).state.export(
            &dir,
            &export.name.text,
            &sources,
            export.options,
        );
        export.step = match started {
            Ok(running) => Step::Running(running),
            Err(detail) => Step::Failed {
                title: "Couldn't export".into(),
                detail,
            },
        };
        export.typing = None;
        window.focus(&self.focus, context);
    }

    /// Goes on to the result once the files are rendered.
    fn poll_export(&mut self) {
        let Step::Running(running) = &mut self.export.step else {
            return;
        };
        let Some(outcome) = running.outcome().cloned() else {
            return;
        };
        self.export.step = match outcome {
            ExportOutcome::Done => Step::Done {
                files: running.files().to_vec(),
                waveforms: running.waveforms(),
                seconds: running.seconds(),
            },
            ExportOutcome::Cancelled => Step::Options,
            ExportOutcome::Failed(detail) => Step::Failed {
                title: "Couldn't export".into(),
                detail,
            },
        };
    }

    /// The export dialog at its step.
    pub(super) fn export_dialog(
        &mut self,
        window: &Window,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        self.poll_export();
        match &self.export.step {
            Step::Options => self.export_options(window, context),
            Step::Running(running) => self.export_running(running, context),
            Step::Done {
                files,
                waveforms,
                seconds,
            } => self.export_done(files, waveforms, *seconds, context),
            Step::Failed { title, detail } => self.export_failed(title, detail, context),
        }
    }

    /// Starts moving the exported part's bound nearest the pointer, which
    /// goes there: the range becomes a custom one.
    fn preview_down(&mut self, event: &MouseDownEvent, context: &mut App) {
        let Some(at) = self.preview_beat(f32::from(event.position.x), context) else {
            return;
        };
        let (start, end) = self
            .state(context)
            .export_span(self.export.options.range)
            .ok()
            .or(self.export.custom)
            .unwrap_or_default();
        let bound = if (at.0 - start.0).abs() <= (at.0 - end.0).abs() {
            Bound::Start
        } else {
            Bound::End
        };
        self.export.custom = Some((start, end));
        self.export.options.range = ExportRange::Custom(start, end);
        self.export.dragged_bound = Some(bound);
        self.move_bound(bound, at);
    }

    /// Follows a drag in the preview. Returns whether it's one.
    pub(super) fn export_drag_move(&mut self, event: &MouseMoveEvent, context: &mut App) -> bool {
        let Some(bound) = self.export.dragged_bound else {
            return false;
        };
        if let Some(at) = self.preview_beat(f32::from(event.position.x), context) {
            self.move_bound(bound, at);
        }
        true
    }

    /// Moves one bound of the custom range to `at`, kept a beat from the
    /// other.
    fn move_bound(&mut self, bound: Bound, at: BeatPos) {
        let (start, end) = self.export.custom.unwrap_or_default();
        self.export.set_custom(match bound {
            Bound::Start => (BeatPos(at.0.min(end.0 - 1.).max(0.)), end),
            Bound::End => (start, BeatPos(at.0.max(start.0 + 1.))),
        });
    }

    pub(super) fn export_drag_end(&mut self) {
        self.export.dragged_bound = None;
    }
}
