//! New, open and save: file dialogs, the unsaved-changes prompt, the window
//! title and closing the window.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use egui::{Align, Key, Layout, Ui, ViewportCommand, vec2};
use rfd::FileDialog;

use crate::{
    config::{config_dir, recent::RecentProjects},
    core::{
        project::{EXTENSION, ProjectFile, project_name},
        state::ProjectState,
    },
    ui::widget::dialog,
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
    /// Another project replaced the current one since last asked.
    replaced: bool,
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
            replaced: false,
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

    /// Whether another project replaced the current one since last asked:
    /// what refers to its clips and tracks no longer does.
    pub fn take_replaced(&mut self) -> bool {
        std::mem::take(&mut self.replaced)
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
                self.replaced = true;
                self.path = None;
                self.saved = state.project(None);
            }
            Pending::Open(path) => self.open(path, state),
            Pending::Quit => self.closing = true,
        }
    }

    /// Open `path`, or the project the user picks.
    fn open(&mut self, path: Option<PathBuf>, state: &mut ProjectState) {
        let Some(path) = path.or_else(|| pick(project_dialog(self.dir()))) else {
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
        self.replaced = true;
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
        let Some(mut path) = pick_save(
            project_dialog(self.dir()).set_file_name(format!("{}.{EXTENSION}", self.name())),
        ) else {
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

    /// Save unsaved changes to a new file after a crash: next to the
    /// project, or with the settings if it was never saved or that folder
    /// can't be written. Returns where, or `None` if nothing was unsaved.
    pub fn save_recovery(&self, state: &ProjectState) -> Result<Option<PathBuf>, String> {
        if !self.is_modified(state) {
            return Ok(None);
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let file = format!("{} (recovered {stamp}).{EXTENSION}", self.name());
        let dirs = self
            .dir()
            .map(Path::to_path_buf)
            .into_iter()
            .chain(config_dir().map(|dir| dir.join("recovery")));
        let mut errors = Vec::new();
        for dir in dirs {
            let path = dir.join(&file);
            let written = fs::create_dir_all(&dir)
                .map_err(|e| e.to_string())
                .and_then(|()| state.project(Some(&dir)).write(&path));
            match written {
                Ok(()) => return Ok(Some(path)),
                Err(e) => errors.push(format!("{}: {e}", dir.display())),
            }
        }
        if errors.is_empty() {
            errors.push("no folder to save it in".into());
        }
        Err(errors.join("; "))
    }

    /// Window title, closing the window, the prompt and notices.
    pub fn ui(&mut self, ui: &mut Ui, state: &mut ProjectState) {
        let modified = self.is_modified(state);
        let title = format!(
            "{}{} — Tonique",
            self.name(),
            if modified { " *" } else { "" }
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
        let mut answer = None;
        let modal = dialog::modal(ui, "unsaved-changes").show(ui.ctx(), |ui| {
            ui.set_width(340.);
            ui.spacing_mut().item_spacing = vec2(8., 6.);
            dialog::title(ui, format!("Save changes to “{}”?", self.name()));
            dialog::body(ui, "Your changes will be lost if you don't save them.");
            ui.add_space(12.);
            // Saving on the right, where the eye ends; losing the changes
            // apart on the left, so it isn't clicked by mistake.
            ui.horizontal(|ui| {
                if ui.add(dialog::button("Don't save")).clicked() {
                    answer = Some(false);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let save = ui.add(dialog::button("Save").selected(true));
                    if save.clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        answer = Some(true);
                    }
                    if ui.add(dialog::button("Cancel")).clicked() {
                        self.pending = None;
                    }
                });
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
        let mut close = false;
        let modal = dialog::modal(ui, "project-notice").show(ui.ctx(), |ui| {
            ui.set_width(360.);
            ui.spacing_mut().item_spacing = vec2(8., 6.);
            dialog::title(ui, &notice.title);
            egui::ScrollArea::vertical()
                .max_height(200.)
                .show(ui, |ui| {
                    for line in &notice.lines {
                        dialog::body(ui, line);
                    }
                });
            ui.add_space(12.);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let ok = ui.add(dialog::button("OK").selected(true));
                close = ok.clicked() || ui.input(|i| i.key_pressed(Key::Enter));
            });
        });
        if close || modal.should_close() {
            self.notice = None;
        }
    }
}

/// Picked in a native dialog; none in tests, where it would wait for a click.
fn pick(dialog: FileDialog) -> Option<PathBuf> {
    if cfg!(test) { None } else { dialog.pick_file() }
}

/// Picked in a native dialog; none in tests.
fn pick_save(dialog: FileDialog) -> Option<PathBuf> {
    if cfg!(test) { None } else { dialog.save_file() }
}

fn project_dialog(dir: Option<&Path>) -> FileDialog {
    let dialog = FileDialog::new().add_filter("Tonique project", &[EXTENSION]);
    match dir {
        Some(dir) => dialog.set_directory(dir),
        None => dialog,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonique_engine::engine::{Engine, EngineConfig};

    /// After a crash, unsaved changes land in a new file next to the
    /// project; nothing is written when there were none.
    #[test]
    fn recovery_saves_unsaved_changes_next_to_the_project() {
        let dir = std::env::temp_dir().join(format!("tonique-recovery-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        let mut manager = ProjectManager::new(&state);
        manager.path = Some(dir.join(format!("song.{EXTENSION}")));
        assert_eq!(manager.save_recovery(&state), Ok(None));

        state.add_track();
        let path = manager.save_recovery(&state).ok().flatten().expect("saved");
        assert_eq!(path.parent(), Some(dir.as_path()));
        assert!(path.to_string_lossy().contains("song (recovered"));
        let recovered = ProjectFile::read(&path).unwrap();
        assert_eq!(recovered, state.project(Some(&dir)));
        let _ = fs::remove_dir_all(&dir);
    }
}
