//! New, open and save, as the egui app's `ProjectManager`: file pickers,
//! the prompt about unsaved changes (also on closing the window), notices
//! about what went wrong, the recent projects and the window's title.

use std::path::{Path, PathBuf};

use gpui::{Context, PathPromptOptions, PromptLevel, Task, Window};

use crate::{
    app::Tonique,
    config::recent::RecentProjects,
    core::{
        project::{EXTENSION, ProjectFile, project_name},
        state::ProjectState,
    },
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
    /// The user agreed to close: let the window go.
    closing: bool,
    /// The window's title, as last set.
    title: String,
}

impl ProjectFiles {
    pub fn new(state: &ProjectState) -> Self {
        Self {
            path: None,
            saved: state.project(None),
            recent: RecentProjects::load(),
            closing: false,
            title: String::new(),
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

    fn remember(&mut self, path: &Path) {
        self.recent.add(path);
        self.recent.save();
    }

    fn forget(&mut self, path: &Path) {
        self.recent.remove(path);
        self.recent.save();
    }
}

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
        let message = format!("Save changes to “{}”?", self.project.name());
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some("Your changes will be lost if you don't save them."),
            &["Save", "Don't save", "Cancel"],
            context,
        );
        context
            .spawn_in(window, async move |this, context| {
                let Ok(answer) = answer.await else { return };
                let go_on = match answer {
                    0 => {
                        let Ok(save) = this.update_in(context, |this, window, context| {
                            this.save_project(window, context)
                        }) else {
                            return;
                        };
                        // Not saved (no file chosen, or it failed): stay.
                        save.await
                    }
                    1 => true,
                    _ => false,
                };
                if go_on {
                    this.update_in(context, |this, window, context| {
                        this.run_after_prompt(then, window, context);
                        context.notify();
                    })
                    .ok();
                }
            })
            .detach();
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
                self.project.saved = self.state.project(None);
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
        self.project.saved = self.state.project(Some(&dir));
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
                self.project.saved = project;
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

    /// The window's title: the project's name, starred while unsaved.
    pub(super) fn update_title(&mut self, window: &mut Window) {
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
