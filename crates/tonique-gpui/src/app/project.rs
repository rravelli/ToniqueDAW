//! New, open and save, as the egui app's `ProjectManager`: file pickers,
//! the prompt about unsaved changes (also on closing the window), notices
//! about what went wrong, the recent projects and the window's title.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use gpui::{
    Context, MouseButton, PathPromptOptions, PromptLevel, Stateful, Task, Window, div, prelude::*,
    px,
};

use crate::{
    app::{Tonique, dialog::Dialog},
    config::recent::RecentProjects,
    core::{
        project::{EXTENSION, ProjectFile, project_name},
        state::ProjectState,
    },
    widgets::{dialog_body, dialog_button, dialog_frame, dialog_title},
};

/// What the user asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum ProjectAction {
    New,
    /// Choose a project to open.
    Open,
    /// Open this recent project.
    OpenRecent(PathBuf),
    Save,
    SaveAs,
    ClearRecent,
}

/// What waits for the unsaved changes to be saved or let go.
#[derive(Clone, Debug, PartialEq)]
enum AfterPrompt {
    New,
    /// A project to open, or `None` to choose one.
    Open(Option<PathBuf>),
    Close,
}

pub struct ProjectFiles {
    /// `None` until first saved or opened.
    path: Option<PathBuf>,
    /// The project as last saved or opened: anything different is unsaved.
    saved: ProjectFile,
    recent: RecentProjects,
    /// What waits for the user's answer about unsaved changes.
    pending: Option<AfterPrompt>,
    /// The user agreed to close: let the window go.
    closing: bool,
    /// The window's title, as last set.
    title: String,
    /// When the title last looked for unsaved changes: comparing the
    /// project with the saved one takes too long to do every frame.
    title_checked: Option<Instant>,
}

/// How long the title's star may lag an edit.
const TITLE_CHECK_INTERVAL: Duration = Duration::from_millis(250);

impl ProjectFiles {
    pub fn new(state: &ProjectState) -> Self {
        Self {
            path: None,
            saved: state.project(None),
            recent: RecentProjects::load(),
            pending: None,
            closing: false,
            title: String::new(),
            title_checked: None,
        }
    }

    /// The project's folder, once saved or opened.
    pub fn dir(&self) -> Option<&Path> {
        self.path.as_deref().and_then(Path::parent)
    }

    /// Recently opened or saved projects, newest first.
    pub fn recent(&self) -> &[PathBuf] {
        self.recent.paths()
    }

    pub fn name(&self) -> String {
        self.path.as_deref().map_or("Untitled".into(), project_name)
    }

    pub fn is_modified(&self, state: &ProjectState) -> bool {
        state.project(self.dir()) != self.saved
    }

    /// `project` as saved or opened: the title shows it now.
    fn set_saved(&mut self, project: ProjectFile) {
        self.saved = project;
        self.title_checked = None;
    }

    fn remember(&mut self, path: &Path) {
        self.recent.add(path);
        self.recent.save();
    }

    fn forget(&mut self, path: &Path) {
        self.recent.remove(path);
        self.recent.save();
    }
}

/// The egui dialog's 340 wide content, with the frame's margins and border.
const UNSAVED_WIDTH: f32 = 340. + 2. * 18. + 2.;

/// Where file pickers start: the project's folder, else the home folder.
fn start_dir(files: &ProjectFiles) -> PathBuf {
    files
        .dir()
        .map(Path::to_path_buf)
        .or_else(std::env::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

impl Tonique {
    pub fn project_action(
        &mut self,
        action: ProjectAction,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        match action {
            ProjectAction::New => self.after_prompt(AfterPrompt::New, window, context),
            ProjectAction::Open => self.after_prompt(AfterPrompt::Open(None), window, context),
            ProjectAction::OpenRecent(path) => {
                self.after_prompt(AfterPrompt::Open(Some(path)), window, context)
            }
            ProjectAction::Save => self.save_project(window, context).detach(),
            ProjectAction::SaveAs => self.save_project_as(window, context).detach(),
            ProjectAction::ClearRecent => {
                self.project.recent.clear();
                self.project.recent.save();
            }
        }
    }

    /// Runs `then` now, or once the user answered about unsaved changes:
    /// save them, let them go, or cancel.
    fn after_prompt(
        &mut self,
        then: AfterPrompt,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        if !self.project.is_modified(&self.state) {
            self.run_after_prompt(then, window, context);
            return;
        }
        self.project.pending = Some(then);
        self.dialog = Some(Dialog::Unsaved);
        // Enter answers it, not a focused list or field.
        window.focus(&self.focus, context);
    }

    /// The answer about unsaved changes: save them first (`true`) or let
    /// them go, then run what waited.
    pub(super) fn answer_unsaved(
        &mut self,
        save: bool,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        self.dialog = None;
        let Some(then) = self.project.pending.take() else {
            return;
        };
        if !save {
            self.run_after_prompt(then, window, context);
            return;
        }
        let saved = self.save_project(window, context);
        context
            .spawn_in(window, async move |this, context| {
                // Not saved (no file chosen, or it failed): stay.
                if !saved.await {
                    return;
                }
                this.update_in(context, |this, window, context| {
                    this.run_after_prompt(then, window, context);
                    context.notify();
                })
                .ok();
            })
            .detach();
    }

    /// The question about unsaved changes, as the egui app's: saving on
    /// the right, where the eye ends; losing the changes apart on the
    /// left, so it isn't clicked by mistake.
    pub(super) fn unsaved_dialog(&self, context: &mut Context<Self>) -> gpui::Div {
        let theme = &*crate::theme::current(context);
        let answer = |button: Stateful<gpui::Div>, save: Option<bool>| {
            button.on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, _, window, context| {
                    match save {
                        Some(save) => this.answer_unsaved(save, window, context),
                        None => this.close_dialog(window, context),
                    }
                    context.notify();
                }),
            )
        };
        let dont_save = answer(
            dialog_button("dont-save", "Don't save", false, theme),
            Some(false),
        );
        let cancel = answer(dialog_button("cancel", "Cancel", false, theme), None);
        let save = answer(dialog_button("save", "Save", true, theme), Some(true));
        dialog_frame(UNSAVED_WIDTH, theme)
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(dialog_title(format!(
                "Save changes to “{}”?",
                self.project.name()
            )))
            .child(dialog_body(
                "Your changes will be lost if you don't save them.",
                theme,
            ))
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(dont_save)
                    .child(div().flex_1())
                    .child(cancel)
                    .child(save),
            )
    }

    fn run_after_prompt(
        &mut self,
        then: AfterPrompt,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        match then {
            AfterPrompt::New => {
                self.state.new_project();
                self.project.path = None;
                self.project.set_saved(self.state.project(None));
                self.forget_old_project();
            }
            AfterPrompt::Open(Some(path)) => self.open_project(&path, window, context),
            AfterPrompt::Open(None) => self.pick_project(window, context),
            AfterPrompt::Close => {
                self.project.closing = true;
                window.remove_window();
            }
        }
    }

    /// Lets the user pick a project, then opens it.
    fn pick_project(&mut self, window: &mut Window, context: &mut Context<Self>) {
        let picked = context.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open".into()),
        });
        context
            .spawn_in(window, async move |this, context| {
                let Ok(Ok(Some(paths))) = picked.await else {
                    return;
                };
                let Some(path) = paths.into_iter().next() else {
                    return;
                };
                this.update_in(context, |this, window, context| {
                    this.open_project(&path, window, context);
                    context.notify();
                })
                .ok();
            })
            .detach();
    }

    fn open_project(&mut self, path: &Path, window: &mut Window, context: &mut Context<Self>) {
        let project = match ProjectFile::read(path) {
            Ok(project) => project,
            Err(error) => {
                if !path.exists() {
                    self.project.forget(path);
                }
                let title = format!("Couldn't open “{}”", project_name(path));
                self.notice(&title, &[error], window, context);
                return;
            }
        };
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut problems = self.state.load_project(&project, &dir);
        self.project.set_saved(self.state.project(Some(&dir)));
        self.project.remember(path);
        self.project.path = Some(path.to_path_buf());
        self.forget_old_project();
        let missing = self.state.missing_files().len();
        if missing > 0 {
            self.show_missing_files(None);
            problems.push(format!(
                "{missing} audio file{} not found: see the browser's Missing tab.",
                if missing == 1 { " was" } else { "s were" }
            ));
        }
        if !problems.is_empty() {
            let title = format!("“{}” opened with problems", project_name(path));
            self.notice(&title, &problems, window, context);
        }
    }

    /// Saves to the project's file, or asks where. The task tells whether
    /// it saved.
    fn save_project(&mut self, window: &mut Window, context: &mut Context<Self>) -> Task<bool> {
        match self.project.path.clone() {
            Some(path) => Task::ready(self.write_project(&path, window, context)),
            None => self.save_project_as(window, context),
        }
    }

    fn save_project_as(&mut self, window: &mut Window, context: &mut Context<Self>) -> Task<bool> {
        let name = format!("{}.{EXTENSION}", self.project.name());
        let picked = context.prompt_for_new_path(&start_dir(&self.project), Some(&name));
        context.spawn_in(window, async move |this, context| {
            let Ok(Ok(Some(mut path))) = picked.await else {
                return false;
            };
            if path
                .extension()
                .is_none_or(|extension| extension != EXTENSION)
            {
                path.set_extension(EXTENSION);
            }
            this.update_in(context, |this, window, context| {
                this.write_project(&path, window, context)
            })
            .unwrap_or(false)
        })
    }

    fn write_project(
        &mut self,
        path: &Path,
        window: &mut Window,
        context: &mut Context<Self>,
    ) -> bool {
        let project = self.state.project(path.parent());
        match project.write(path) {
            Ok(()) => {
                self.project.remember(path);
                self.project.path = Some(path.to_path_buf());
                self.project.set_saved(project);
                true
            }
            Err(error) => {
                let title = format!("Couldn't save “{}”", project_name(path));
                self.notice(&title, &[error], window, context);
                false
            }
        }
    }

    /// Tells the user what went wrong, until dismissed.
    fn notice(
        &self,
        title: &str,
        lines: &[String],
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let detail = lines.join("\n");
        // Only dismissed: the answer doesn't matter.
        drop(window.prompt(
            PromptLevel::Critical,
            title,
            Some(&detail),
            &["OK"],
            context,
        ));
    }

    /// What referred to the replaced project's tracks and clips no longer
    /// does.
    fn forget_old_project(&mut self) {
        self.popup = None;
        self.headers.forget_project();
        self.timeline.forget_project();
        self.bottom.forget_project();
    }

    /// The window's title: the project's name, starred while unsaved;
    /// looked at a few times a second.
    pub(super) fn update_title(&mut self, window: &mut Window) {
        let now = Instant::now();
        if self
            .project
            .title_checked
            .is_some_and(|checked| now - checked < TITLE_CHECK_INTERVAL)
        {
            return;
        }
        self.project.title_checked = Some(now);
        let modified = self.project.is_modified(&self.state);
        let title = format!(
            "{}{} — Tonique",
            self.project.name(),
            if modified { " *" } else { "" }
        );
        if title != self.project.title {
            window.set_window_title(&title);
            self.project.title = title;
        }
    }

    /// Asks about unsaved changes before the window closes. Returns
    /// whether it can close now.
    pub(super) fn should_close(
        &mut self,
        window: &mut Window,
        context: &mut Context<Self>,
    ) -> bool {
        if self.project.closing || !self.project.is_modified(&self.state) {
            return true;
        }
        self.after_prompt(AfterPrompt::Close, window, context);
        false
    }
}
