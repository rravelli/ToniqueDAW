//! New, open and save as the core's `ProjectFiles` runs them; here, their
//! dialogs: the file pickers, the unsaved-changes prompt, the notices,
//! the window title and closing the window.

use std::path::{Path, PathBuf};

use egui::{Align, Key, Layout, Ui, ViewportCommand, vec2};
use rfd::FileDialog;

use crate::ui::{missing_files, widget::dialog};
pub use tonique_core::project::files::ProjectAction;
use tonique_core::{
    project::EXTENSION,
    project::files::{Ask, FileOutcome, Notice, ProjectFiles},
    state::ProjectState,
};

pub struct ProjectManager {
    files: ProjectFiles,
    notice: Option<Notice>,
    /// A project with missing files was opened: show them.
    show_missing: bool,
    /// Set once the user agreed to close, to let the window go.
    closing: bool,
    title: String,
    /// Another project replaced the current one since last asked.
    replaced: bool,
}

impl ProjectManager {
    pub fn new(state: &ProjectState) -> Self {
        Self {
            files: ProjectFiles::new(state),
            notice: None,
            show_missing: false,
            closing: false,
            title: String::new(),
            replaced: false,
        }
    }

    /// The project's folder, once saved or opened.
    pub fn dir(&self) -> Option<&Path> {
        self.files.dir()
    }

    /// Recently opened or saved projects, newest first.
    pub fn recent(&self) -> &[PathBuf] {
        self.files.recent()
    }

    pub fn name(&self) -> String {
        self.files.name()
    }

    /// Whether another project replaced the current one since last asked:
    /// what refers to its clips and tracks no longer does.
    pub fn take_replaced(&mut self) -> bool {
        std::mem::take(&mut self.replaced)
    }

    pub fn request(&mut self, action: ProjectAction, state: &mut ProjectState) {
        let outcome = self.files.request(action, state);
        self.follow(outcome, state);
    }

    /// See [`ProjectFiles::save_recovery`].
    pub fn save_recovery(&self, state: &ProjectState) -> Result<Option<PathBuf>, String> {
        self.files.save_recovery(state)
    }

    /// Does what a step left to the UI; the native pickers answer at once.
    fn follow(&mut self, outcome: FileOutcome, state: &mut ProjectState) {
        self.replaced |= outcome.replaced;
        self.show_missing |= outcome.missing_files > 0;
        self.closing |= outcome.close;
        if outcome.notice.is_some() {
            self.notice = outcome.notice;
        }
        let next = match outcome.ask {
            // Drawn while the files wait for the answer.
            None | Some(Ask::Unsaved) => return,
            Some(Ask::ProjectToOpen { start_dir }) => {
                let path = pick(project_dialog(&start_dir));
                self.files.opened(path, state)
            }
            Some(Ask::SavePath {
                start_dir,
                file_name,
            }) => {
                let path = pick_save(project_dialog(&start_dir).set_file_name(file_name));
                self.files.saved_to(path, state)
            }
        };
        self.follow(next, state);
    }

    /// Window title, closing the window, the prompt and notices.
    pub fn ui(&mut self, ui: &mut Ui, state: &mut ProjectState) {
        let title = self.files.title(state);
        if title != self.title {
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        // Closing the window: ask first when there are unsaved changes.
        if ui.input(|i| i.viewport().close_requested()) && !self.files.should_close(state) {
            ui.ctx().send_viewport_cmd(ViewportCommand::CancelClose);
        }
        if self.closing {
            ui.ctx().send_viewport_cmd(ViewportCommand::Close);
        }

        if self.files.asking_unsaved() {
            self.prompt(ui, state);
        }
        self.notice(ui);
        if std::mem::take(&mut self.show_missing) {
            missing_files::request(ui.ctx(), None);
        }
    }

    fn prompt(&mut self, ui: &Ui, state: &mut ProjectState) {
        let mut answer = None;
        let mut cancel = false;
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
                        cancel = true;
                    }
                });
            });
        });
        if cancel || modal.should_close() {
            answer = None;
        } else if answer.is_none() {
            return;
        }
        let outcome = self.files.answer_unsaved(answer, state);
        self.follow(outcome, state);
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

fn project_dialog(dir: &Path) -> FileDialog {
    FileDialog::new()
        .add_filter("Tonique project", &[EXTENSION])
        .set_directory(dir)
}
