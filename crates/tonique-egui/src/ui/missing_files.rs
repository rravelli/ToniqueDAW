//! Audio files a project uses that weren't found, in a view of the left
//! panel: each looked for in the browser's folders by name, to replace
//! with one of the closest matches, or picked on disk. Their clips are
//! kept, silent, until then.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use egui::{Context, Id, RichText, ScrollArea, Ui};
use egui_phosphor::regular::{CHECK_CIRCLE, WARNING, WAVEFORM};
use rfd::FileDialog;
use tonique_engine::edit::SourceId;

use crate::ui::{
    theme::{ThemeExt, font_size},
    widget::{dialog, list_row::ListRow, search_bar::SearchBar},
};
use tonique_core::{
    library::file_match::{
        AUDIO_EXTENSIONS, FileIndex, exact_matches, file_name, folder_name, folder_of, query_for,
        rank,
    },
    project::ProjectState,
};

/// Asks the view to show, from anywhere (a clip's menu, the transport
/// bar, opening a project): the left panel does on the next frame.
#[derive(Clone, Default)]
struct Request {
    /// The file to select: a missing clip's.
    path: Option<PathBuf>,
}

/// Show the missing files in the left panel, `path`'s selected if given.
pub fn request(ctx: &Context, path: Option<PathBuf>) {
    ctx.data_mut(|d| d.insert_temp(request_id(), Request { path }));
}

fn request_id() -> Id {
    Id::new("missing-files-request")
}

/// Whether the view was asked to show since last asked, and the file to
/// select if one was given.
pub fn take_request(ctx: &Context) -> Option<Option<PathBuf>> {
    ctx.data_mut(|d| d.remove_temp::<Request>(request_id()))
        .map(|r| r.path)
}

/// The view: the missing files, and the matches for the selected one.
pub struct MissingFilesView {
    selected: Option<SourceId>,
    search: SearchBar,
    index: FileIndex,
    /// What the last replacement did.
    status: Option<Status>,
}

enum Status {
    Located(usize),
    Failed(String),
}

impl MissingFilesView {
    pub fn new() -> Self {
        Self {
            selected: None,
            search: SearchBar::new("Search your folders"),
            index: FileIndex::default(),
            status: None,
        }
    }

    /// Select the missing file at `path`.
    pub fn select_path(&mut self, state: &ProjectState, path: &Path) {
        let found = state
            .missing_files()
            .into_iter()
            .find(|(_, audio)| audio.path == path)
            .map(|(id, audio)| (id, audio.path.clone()));
        if let Some((id, path)) = found {
            self.select(id, &path);
        }
    }

    fn select(&mut self, id: SourceId, path: &Path) {
        self.selected = Some(id);
        self.search.set_query(query_for(path));
    }

    /// The search bar, at the panel's top.
    pub fn header_ui(&mut self, ui: &mut Ui, folders: &[PathBuf]) {
        self.index.update(folders);
        self.search.set_busy(self.index.is_scanning());
        ui.push_id("missing-search", |ui| self.search.ui(ui));
    }

    /// `folders`: the browser's, searched for the files.
    pub fn ui(&mut self, ui: &mut Ui, state: &mut ProjectState, folders: &[PathBuf]) {
        let theme = ui.app_theme();
        let missing: Vec<(SourceId, String, PathBuf)> = state
            .missing_files()
            .into_iter()
            .map(|(id, audio)| (id, audio.name.clone(), audio.path.clone()))
            .collect();
        if missing.is_empty() {
            self.selected = None;
            ui.add_space(12.);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(CHECK_CIRCLE)
                        .size(20.)
                        .color(theme.text_muted),
                );
                note(ui, "Every audio file is found.");
            });
            return;
        }
        // The first one, unless one still missing is selected.
        if !missing.iter().any(|(id, ..)| Some(*id) == self.selected) {
            let (id, _, path) = &missing[0];
            self.select(*id, &path.clone());
        }

        heading(ui, &format!("Missing ({})", missing.len()));
        let mut clicked = None;
        for (id, name, path) in &missing {
            let row = ui
                .add(
                    ListRow::new(name)
                        .icon(WARNING)
                        .icon_color(theme.warning)
                        .selected(Some(*id) == self.selected),
                )
                .on_hover_text(format!("Was in {}", folder_of(path)));
            if row.clicked() {
                clicked = Some((*id, path.clone()));
            }
        }
        if let Some((id, path)) = clicked {
            self.select(id, &path);
            self.status = None;
        }

        ui.add_space(6.);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.;
            let exact = exact_matches(&missing, self.index.files());
            let find_all = ui
                .add_enabled(!exact.is_empty(), dialog::button("Find all"))
                .on_hover_text(
                    "Replace each missing file by the one of the same name in your folders",
                );
            if find_all.clicked() {
                let mut located = 0;
                for (id, path) in exact {
                    // Already found along with another, from its folder.
                    if state.missing_files().iter().any(|(m, _)| *m == id) {
                        located += state.locate_file(id, &path).unwrap_or(0);
                    }
                }
                self.status = Some(Status::Located(located));
            }
            let locate = ui
                .add(dialog::button("Locate on disk…"))
                .on_hover_text("Pick the file on disk");
            if locate.clicked()
                && let Some(id) = self.selected
                && let Some(path) = pick_file(folders.first().map(PathBuf::as_path))
            {
                self.status = Some(match state.locate_file(id, &path) {
                    Ok(located) => Status::Located(located),
                    Err(error) => Status::Failed(error),
                });
            }
        });
        if let Some(status) = &self.status {
            let (text, color) = match status {
                Status::Located(0) => ("No file found.".to_string(), theme.warning),
                Status::Located(1) => ("Found 1 file.".to_string(), theme.text_muted),
                Status::Located(n) => (format!("Found {n} files."), theme.text_muted),
                Status::Failed(error) => (error.clone(), theme.danger),
            };
            ui.label(RichText::new(text).size(font_size::LABEL).color(color));
        }

        ui.add_space(8.);
        let Some((selected, name, path)) = missing
            .iter()
            .find(|(id, ..)| Some(*id) == self.selected)
            .cloned()
        else {
            return;
        };
        heading(ui, &format!("Replace {name} with"));
        if folders.is_empty() {
            note(ui, "Add your sample folders in the browser to search them.");
            return;
        }
        let matches = rank(&path, self.search.query(), self.index.files());
        if matches.is_empty() {
            note(
                ui,
                if self.index.is_scanning() {
                    "Searching your folders…"
                } else {
                    "Nothing alike in your folders."
                },
            );
            return;
        }
        // Names alike tell their folders.
        let mut names: HashMap<&str, usize> = HashMap::new();
        for found in &matches {
            *names.entry(file_name(found)).or_default() += 1;
        }
        let mut picked = None;
        ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for found in &matches {
                    let name = file_name(found);
                    let text = if names[name] > 1 {
                        format!("{name} · {}", folder_name(found))
                    } else {
                        name.to_string()
                    };
                    let row = ui
                        .add(ListRow::new(text).icon(WAVEFORM))
                        .on_hover_text(format!(
                            "In {}\nClick to use it for every clip of {}",
                            folder_of(found),
                            file_name(&path)
                        ));
                    if row.clicked() {
                        picked = Some(found.clone());
                    }
                }
            });
        if let Some(found) = picked {
            self.status = Some(match state.locate_file(selected, &found) {
                Ok(located) => Status::Located(located),
                Err(error) => Status::Failed(error),
            });
        }
    }
}

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(2.);
    ui.label(
        RichText::new(text.to_uppercase())
            .size(font_size::SMALL)
            .color(ui.app_theme().text_muted),
    );
    ui.add_space(2.);
}

fn note(ui: &mut Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(font_size::LABEL)
                .color(ui.app_theme().text_muted),
        )
        .wrap(),
    );
}

/// Picked in a native dialog; none in tests, where it would wait for a click.
fn pick_file(dir: Option<&Path>) -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    let dialog = FileDialog::new()
        .add_filter("Audio", AUDIO_EXTENSIONS)
        .add_filter("All files", &["*"]);
    match dir {
        Some(dir) => dialog.set_directory(dir),
        None => dialog,
    }
    .pick_file()
}
