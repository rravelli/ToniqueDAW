//! Audio files a project uses that weren't found: listed after opening it,
//! to locate one by one or search for in a folder. Their clips are kept,
//! silent, until then.

use std::path::{Path, PathBuf};

use egui::{Align, Context, FontFamily, Id, Key, Layout, RichText, Ui, vec2};
use egui_phosphor::regular::{CHECK, WARNING};
use rfd::FileDialog;

use crate::{
    core::state::ProjectState,
    ui::{
        font::PHOSPHOR_REGULAR,
        theme::{ThemeExt, font_size},
        widget::dialog,
    },
};

/// Extensions offered when locating a file; any readable file is taken.
const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "ogg", "aiff", "aif", "aac", "m4a"];

/// The dialog, shown while open.
#[derive(Default)]
pub struct MissingFiles {
    open: bool,
    /// What the last search or locate did.
    status: Option<Status>,
}

enum Status {
    Located(usize),
    Failed(String),
}

impl MissingFiles {
    /// Show the dialog if any file is missing.
    pub fn open_if_any(&mut self, state: &ProjectState) {
        self.open = !state.missing_files().is_empty();
        self.status = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.status = None;
    }

    /// Ask the dialog to open from anywhere (a clip's menu): it does on
    /// the next frame.
    pub fn request(ctx: &Context) {
        ctx.data_mut(|d| d.insert_temp(request_id(), true));
    }

    /// `dir`: the project's folder, where file pickers start.
    pub fn ui(&mut self, ui: &mut Ui, state: &mut ProjectState, dir: Option<&Path>) {
        if ui.ctx().data_mut(|d| d.remove_temp::<bool>(request_id())) == Some(true) {
            self.open_if_any(state);
        }
        if !self.open {
            return;
        }
        let missing: Vec<_> = state
            .missing_files()
            .into_iter()
            .map(|(id, audio)| (id, audio.name.clone(), audio.path.clone()))
            .collect();
        let theme = ui.app_theme();
        let mut close = false;
        let mut picked = None;
        let mut search = false;
        let modal = dialog::modal(ui, "missing-files").show(ui.ctx(), |ui| {
            ui.set_width(440.);
            ui.spacing_mut().item_spacing = vec2(8., 6.);
            if missing.is_empty() {
                dialog::title(ui, "All audio files found");
                dialog::body(ui, "Every clip plays its file again.");
            } else {
                dialog::title(ui, "Missing audio files");
                dialog::body(
                    ui,
                    match missing.len() {
                        1 => "A file this project uses wasn't found. \
                              Its clips are kept, silent, until you locate it."
                            .to_string(),
                        n => format!(
                            "{n} files this project uses weren't found. \
                             Their clips are kept, silent, until you locate them."
                        ),
                    },
                );
            }
            ui.add_space(6.);
            egui::ScrollArea::vertical()
                .max_height(240.)
                .show(ui, |ui| {
                    for (id, name, path) in &missing {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.;
                                ui.label(RichText::new(name).size(font_size::BODY));
                                let folder = path.parent().unwrap_or(path);
                                ui.label(
                                    RichText::new(folder.to_string_lossy())
                                        .color(theme.text_muted)
                                        .size(font_size::LABEL),
                                )
                                .on_hover_text(path.to_string_lossy());
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.add(dialog::button("Locate…")).clicked() {
                                    picked = Some((*id, name.clone()));
                                }
                            });
                        });
                    }
                });
            if let Some(status) = &self.status {
                let (icon, color, text) = match status {
                    Status::Located(0) => (WARNING, theme.warning, "No file found.".into()),
                    Status::Located(1) => (CHECK, theme.text_muted, "Found 1 file.".into()),
                    Status::Located(n) => (CHECK, theme.text_muted, format!("Found {n} files.")),
                    Status::Failed(error) => (WARNING, theme.danger, error.clone()),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.;
                    ui.label(
                        RichText::new(icon)
                            .family(FontFamily::Name(PHOSPHOR_REGULAR.into()))
                            .color(color),
                    );
                    ui.label(RichText::new(text).color(color).size(font_size::LABEL));
                });
            }
            ui.add_space(12.);
            ui.horizontal(|ui| {
                if !missing.is_empty()
                    && ui
                        .add(dialog::button("Search folder…"))
                        .on_hover_text("Look for every missing file in a folder and its subfolders")
                        .clicked()
                {
                    search = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let done = ui.add(dialog::button("Done").selected(missing.is_empty()));
                    close = done.clicked() || ui.input(|i| i.key_pressed(Key::Enter));
                });
            });
        });
        if close || modal.should_close() {
            self.close();
            return;
        }
        if let Some((id, name)) = picked
            && let Some(path) = pick_file(dir, &name)
        {
            self.status = Some(match state.locate_file(id, &path) {
                Ok(located) => Status::Located(located),
                Err(error) => Status::Failed(error),
            });
        }
        if search && let Some(folder) = pick_folder(dir) {
            self.status = Some(Status::Located(state.search_files(&folder, true)));
        }
    }
}

fn request_id() -> Id {
    Id::new("missing-files-request")
}

/// Picked in a native dialog; none in tests, where it would wait for a click.
fn pick_file(dir: Option<&Path>, name: &str) -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    let dialog = FileDialog::new()
        .set_title(format!("Locate “{name}”"))
        .add_filter("Audio", AUDIO_EXTENSIONS)
        .add_filter("All files", &["*"]);
    match dir {
        Some(dir) => dialog.set_directory(dir),
        None => dialog,
    }
    .pick_file()
}

fn pick_folder(dir: Option<&Path>) -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    let dialog = FileDialog::new().set_title("Search for missing files in");
    match dir {
        Some(dir) => dialog.set_directory(dir),
        None => dialog,
    }
    .pick_folder()
}
