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

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::{
    App, Bounds, Context, Entity, EventEmitter, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Render, Window, div, prelude::*,
};
use tonique_engine::time::BeatPos;

use crate::{
    model::{HoldsProject, Project},
    wave::WaveCache,
    widgets::{TextField, follow_drag},
};
use tonique_core::{
    export::{Export, ExportFile, ExportOptions, ExportOutcome, ExportRange, Waveform},
    state::ExportSelection,
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

/// The dialog, a view of its own.
pub struct ExportDialog {
    project: Entity<Project>,
    /// The clips' waveforms, shared with the timeline, for the preview.
    waves: Rc<RefCell<WaveCache>>,
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

/// What the dialog asks of the window.
pub enum ExportEvent {
    Close,
}

impl EventEmitter<ExportEvent> for ExportDialog {}

impl HoldsProject for ExportDialog {
    fn project(&self) -> &Entity<Project> {
        &self.project
    }
}

impl ExportDialog {
    /// What its waveforms take.
    pub fn wave_memory(&self) -> crate::wave::WaveMemory {
        self.waves.borrow().memory()
    }

    pub fn new(project: Entity<Project>, context: &mut Context<Self>) -> Self {
        Self {
            project,
            waves: Rc::default(),
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

    fn is_dragging(&self) -> bool {
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

impl ExportDialog {
    /// Asks what to export, into files named `name` in `dir` unless
    /// another folder was chosen. Ignored while an export runs.
    pub fn open(&mut self, name: String, dir: Option<&Path>) {
        if self.is_running() {
            return;
        }
        self.name.text = name;
        if let Some(dir) = dir {
            self.folder.text = dir.display().to_string();
        }
        self.existing = None;
        self.typing = None;
        self.step = Step::Options;
    }

    /// Lets go of a bound being typed: the window closes the dialog.
    fn close(&mut self, window: &mut Window, context: &mut Context<Self>) {
        self.typing = None;
        crate::app::focus_main(window, context);
        context.emit(ExportEvent::Close);
    }

    fn start(&mut self, window: &mut Window, context: &mut Context<Self>) {
        let state = self.state(context);
        let sources = self.selection.sources(state);
        let dir = PathBuf::from(self.folder.text.trim());
        let started = state.export(&dir, &self.name.text, &sources, self.options);
        self.step = match started {
            Ok(running) => Step::Running(running),
            Err(detail) => Step::Failed {
                title: "Couldn't export".into(),
                detail,
            },
        };
        self.typing = None;
        crate::app::focus_main(window, context);
    }

    /// Goes on to the result once the files are rendered.
    fn poll(&mut self) {
        let Step::Running(running) = &mut self.step else {
            return;
        };
        let Some(outcome) = running.outcome().cloned() else {
            return;
        };
        self.step = match outcome {
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

    /// Starts moving the exported part's bound nearest the pointer, which
    /// goes there: the range becomes a custom one.
    fn preview_down(&mut self, event: &MouseDownEvent, context: &mut App) {
        let Some(at) = self.preview_beat(f32::from(event.position.x), context) else {
            return;
        };
        let (start, end) = self
            .state(context)
            .export_span(self.options.range)
            .ok()
            .or(self.custom)
            .unwrap_or_default();
        let bound = if (at.0 - start.0).abs() <= (at.0 - end.0).abs() {
            Bound::Start
        } else {
            Bound::End
        };
        self.custom = Some((start, end));
        self.options.range = ExportRange::Custom(start, end);
        self.dragged_bound = Some(bound);
        self.move_bound(bound, at);
    }

    /// Follows a drag in the preview.
    fn drag_move(&mut self, event: &MouseMoveEvent, context: &mut Context<Self>) {
        let Some(bound) = self.dragged_bound else {
            return;
        };
        if let Some(at) = self.preview_beat(f32::from(event.position.x), context) {
            self.move_bound(bound, at);
        }
    }

    /// Moves one bound of the custom range to `at`, kept a beat from the
    /// other.
    fn move_bound(&mut self, bound: Bound, at: BeatPos) {
        let (start, end) = self.custom.unwrap_or_default();
        self.set_custom(match bound {
            Bound::Start => (BeatPos(at.0.min(end.0 - 1.).max(0.)), end),
            Bound::End => (start, BeatPos(at.0.max(start.0 + 1.))),
        });
    }

    fn drag_end(&mut self, _: &MouseUpEvent, _: &mut Context<Self>) {
        self.dragged_bound = None;
    }
}

impl Render for ExportDialog {
    fn render(&mut self, window: &mut Window, context: &mut Context<Self>) -> impl IntoElement {
        self.waves.borrow_mut().start_frame(window);
        self.poll();
        // The files draw themselves as they render.
        if self.is_running() {
            window.request_animation_frame();
        }
        let content = match &self.step {
            Step::Options => self.options_view(window, context),
            Step::Running(running) => self.running_view(running, context),
            Step::Done {
                files,
                waveforms,
                seconds,
            } => self.done_view(files, waveforms, *seconds, context),
            Step::Failed { title, detail } => self.failed_view(title, detail, context),
        };
        div().child(content).when(self.is_dragging(), |element| {
            element.child(follow_drag(
                context.weak_entity(),
                Self::drag_move,
                Self::drag_end,
            ))
        })
    }
}
