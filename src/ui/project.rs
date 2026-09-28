//! New, open and save: file dialogs, the unsaved-changes prompt, the window
//! title and closing the window.

use std::path::{Path, PathBuf};

use egui::{Id, Modal, RichText, Ui, ViewportCommand, vec2};
use rfd::FileDialog;

use crate::{
    config::recent::RecentProjects,
    core::{
        project::{EXTENSION, ProjectFile, project_name},
        state::ProjectState,
    },
    ui::{theme::ThemeExt, widget::square_button::SquareButton},
};

const BUTTON_HEIGHT: f32 = 22.;

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

/// Waiting for the unsaved-changes prompt.
#[derive(Clone, Debug, PartialEq)]
enum Pending {
    New,
    /// A project to open, or `None` to choose one.
    Open(Option<PathBuf>),
    Quit,
}

/// Shown until dismissed: what went wrong opening or saving.
struct Notice {
    title: String,
    lines: Vec<String>,
}

pub struct ProjectManager {
    /// `None` until first saved or opened.
    path: Option<PathBuf>,
    /// The project as last saved or opened: anything different is unsaved.
    saved: ProjectFile,
    pending: Option<Pending>,
    notice: Option<Notice>,
    recent: RecentProjects,
    /// Set once the user agreed to close, to let the next close through.
    closing: bool,
    title: String,
}

impl ProjectManager {
    pub fn new(state: &ProjectState) -> Self {
        Self {
            path: None,
            saved: state.project(None),
            pending: None,
            notice: None,
            recent: RecentProjects::load(),
            closing: false,
            title: String::new(),
        }
    }

    fn dir(&self) -> Option<&Path> {
        self.path.as_deref().and_then(Path::parent)
    }

    /// Recently opened or saved projects, newest first.
    pub fn recent(&self) -> &[PathBuf] {
        self.recent.paths()
    }

    fn remember(&mut self, path: &Path) {
        self.recent.add(path);
        self.recent.save();
    }

    pub fn name(&self) -> String {
        self.path.as_deref().map_or("Untitled".into(), project_name)
    }

    pub fn is_modified(&self, state: &ProjectState) -> bool {
        state.project(self.dir()) != self.saved
    }

    pub fn request(&mut self, action: ProjectAction, state: &mut ProjectState) {
        match action {
            ProjectAction::New => self.after_prompt(Pending::New, state),
            ProjectAction::Open => self.after_prompt(Pending::Open(None), state),
            ProjectAction::OpenRecent(path) => self.after_prompt(Pending::Open(Some(path)), state),
            ProjectAction::ClearRecent => {
                self.recent.clear();
                self.recent.save();
            }
            ProjectAction::Save => {
                self.save(state);
            }
            ProjectAction::SaveAs => {
                self.save_as(state);
            }
        }
    }

    /// Run `pending` now, or once the user answered the prompt about unsaved
    /// changes.
    fn after_prompt(&mut self, pending: Pending, state: &mut ProjectState) {
        if self.is_modified(state) {
            self.pending = Some(pending);
        } else {
            self.run(pending, state);
        }
    }

    fn run(&mut self, pending: Pending, state: &mut ProjectState) {
        match pending {
            Pending::New => {
                state.new_project();
                self.path = None;
                self.saved = state.project(None);
            }
            Pending::Open(path) => self.open(path, state),
            Pending::Quit => self.closing = true,
        }
    }

    /// Open `path`, or the project the user picks.
    fn open(&mut self, path: Option<PathBuf>, state: &mut ProjectState) {
        let Some(path) = path.or_else(|| project_dialog(self.dir()).pick_file()) else {
            return;
        };
        let project = match ProjectFile::read(&path) {
            Ok(project) => project,
            Err(error) => {
                if !path.exists() {
                    self.recent.remove(&path);
                    self.recent.save();
                }
                self.notice = Some(Notice {
                    title: format!("Couldn't open “{}”", project_name(&path)),
                    lines: vec![error],
                });
                return;
            }
        };
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let problems = state.load_project(&project, &dir);
        self.saved = state.project(Some(&dir));
        if !problems.is_empty() {
            self.notice = Some(Notice {
                title: format!("“{}” opened with problems", project_name(&path)),
                lines: problems,
            });
        }
        self.remember(&path);
        self.path = Some(path);
    }

    /// Save to the current file, or ask where. Returns whether it saved.
    fn save(&mut self, state: &ProjectState) -> bool {
        match self.path.clone() {
            Some(path) => self.write(&path, state),
            None => self.save_as(state),
        }
    }

    fn save_as(&mut self, state: &ProjectState) -> bool {
        let Some(mut path) = project_dialog(self.dir())
            .set_file_name(format!("{}.{EXTENSION}", self.name()))
            .save_file()
        else {
            return false;
        };
        if path.extension().is_none_or(|ext| ext != EXTENSION) {
            path.set_extension(EXTENSION);
        }
        self.write(&path, state)
    }

    fn write(&mut self, path: &Path, state: &ProjectState) -> bool {
        let project = state.project(path.parent());
        match project.write(path) {
            Ok(()) => {
                self.remember(path);
                self.path = Some(path.to_path_buf());
                self.saved = project;
                true
            }
            Err(error) => {
                self.notice = Some(Notice {
                    title: format!("Couldn't save “{}”", project_name(path)),
                    lines: vec![error],
                });
                false
            }
        }
    }

    /// Window title, closing the window, the prompt and notices.
    pub fn ui(&mut self, ui: &mut Ui, state: &mut ProjectState) {
        let modified = self.is_modified(state);
        let title = format!(
            "{}{} — Tonique",
            self.name(),
            if modified { " •" } else { "" }
        );
        if title != self.title {
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        // Closing the window: ask first when there are unsaved changes.
        if ui.input(|i| i.viewport().close_requested()) && modified && !self.closing {
            ui.ctx().send_viewport_cmd(ViewportCommand::CancelClose);
            self.pending = Some(Pending::Quit);
        }
        if self.closing {
            ui.ctx().send_viewport_cmd(ViewportCommand::Close);
        }

        if let Some(pending) = self.pending.clone() {
            self.prompt(ui, pending, state);
        }
        self.notice(ui);
    }

    fn prompt(&mut self, ui: &Ui, pending: Pending, state: &mut ProjectState) {
        let theme = ui.app_theme();
        let mut answer = None;
        let modal = Modal::new(Id::new("unsaved-changes")).show(ui.ctx(), |ui| {
            ui.set_width(320.);
            ui.spacing_mut().item_spacing = vec2(6., 8.);
            ui.label(RichText::new(format!("Save changes to “{}”?", self.name())).strong());
            ui.label(
                RichText::new("Your changes will be lost if you don't save them.")
                    .color(theme.text_muted),
            );
            ui.horizontal(|ui| {
                if ui.add(button("Save").selected(true)).clicked() {
                    answer = Some(true);
                }
                if ui.add(button("Don't save")).clicked() {
                    answer = Some(false);
                }
                if ui.add(button("Cancel")).clicked() {
                    self.pending = None;
                }
            });
        });
        if modal.should_close() {
            self.pending = None;
        }
        match answer {
            // Saving can be cancelled (no file chosen) or fail: stay put.
            Some(true) if !self.save(state) => self.pending = None,
            Some(_) => {
                self.pending = None;
                self.run(pending, state);
            }
            None => {}
        }
    }

    fn notice(&mut self, ui: &Ui) {
        let Some(notice) = &self.notice else {
            return;
        };
        let theme = ui.app_theme();
        let mut close = false;
        let modal = Modal::new(Id::new("project-notice")).show(ui.ctx(), |ui| {
            ui.set_width(360.);
            ui.spacing_mut().item_spacing = vec2(6., 6.);
            ui.label(RichText::new(&notice.title).strong());
            egui::ScrollArea::vertical()
                .max_height(200.)
                .show(ui, |ui| {
                    for line in &notice.lines {
                        ui.label(RichText::new(line).color(theme.text_muted));
                    }
                });
            close = ui.add(button("OK").selected(true)).clicked();
        });
        if close || modal.should_close() {
            self.notice = None;
        }
    }
}

fn project_dialog(dir: Option<&Path>) -> FileDialog {
    let dialog = FileDialog::new().add_filter("Tonique project", &[EXTENSION]);
    match dir {
        Some(dir) => dialog.set_directory(dir),
        None => dialog,
    }
}

fn button(text: &str) -> SquareButton {
    SquareButton::new(text)
        .size(vec2(0., BUTTON_HEIGHT))
        .padding(10.)
        .font(egui::FontId::proportional(12.))
        .border_radius(2.)
}
