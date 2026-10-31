//! Audio files a project uses that weren't found, in a view of the left
//! panel: each looked for in the browser's folders by name, to replace
//! with one of the closest matches, or picked on disk. Their clips are
//! kept, silent, until then.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, channel},
};

use egui::{Context, Id, RichText, ScrollArea, Ui};
use egui_phosphor::regular::{CHECK_CIRCLE, WARNING, WAVEFORM};
use rfd::FileDialog;
use tonique_engine::edit::SourceId;

use crate::{
    core::state::ProjectState,
    ui::{
        browser::file_tree::is_audio_file,
        theme::{ThemeExt, font_size},
        widget::{dialog, list_row::ListRow, search_bar::SearchBar},
    },
};

/// Extensions offered when picking a file on disk; any readable file is
/// taken.
const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "ogg", "aiff", "aif", "aac", "m4a"];
/// Most matches listed for a file.
const MAX_MATCHES: usize = 40;
/// Most files read from the browser's folders: enough for a sample
/// library, not a whole disk.
const MAX_INDEXED: usize = 300_000;

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
            let exact = self.exact_matches(&missing);
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

    /// For each missing file, the file of the same name in the browser's
    /// folders, if there's one: the one in a folder of the same name when
    /// there are several.
    fn exact_matches(&self, missing: &[(SourceId, String, PathBuf)]) -> Vec<(SourceId, PathBuf)> {
        missing
            .iter()
            .filter_map(|(id, name, path)| {
                let same: Vec<&PathBuf> = self
                    .index
                    .files()
                    .iter()
                    .filter(|found| file_name(found).eq_ignore_ascii_case(name))
                    .collect();
                let found = match same.as_slice() {
                    [one] => *one,
                    many => many
                        .iter()
                        .find(|found| folder_name(found) == folder_name(path))?,
                };
                Some((*id, found.clone()))
            })
            .collect()
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

fn file_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
}

fn folder_name(path: &Path) -> &str {
    path.parent()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str())
        .unwrap_or_default()
}

fn folder_of(path: &Path) -> String {
    path.parent().unwrap_or(path).display().to_string()
}

/// What a missing file is searched for by: its name, without extension.
fn query_for(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Lowercase words and numbers of `text`: `FL_140BPM Kit-5` gives `fl`,
/// `140`, `bpm`, `kit`, `5`.
fn words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut digits = false;
    for c in text.chars() {
        if !c.is_alphanumeric() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            continue;
        }
        if !word.is_empty() && c.is_ascii_digit() != digits {
            words.push(std::mem::take(&mut word));
        }
        digits = c.is_ascii_digit();
        word.extend(c.to_lowercase());
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Files of `files` like `missing`, as searched for by `query`, best
/// first: its very name, then names with every word of the query, then
/// names with most of them; in a folder named as `missing`'s first among
/// equals.
fn rank(missing: &Path, query: &str, files: &[PathBuf]) -> Vec<PathBuf> {
    let wanted = words(query);
    if wanted.is_empty() {
        return Vec::new();
    }
    let name = file_name(missing);
    let folder = folder_name(missing);
    // Searching by the file's own name, names with most of its words are
    // worth a look; what's typed is meant whole.
    let loose = query == query_for(missing);
    let mut scored: Vec<(u32, &PathBuf)> = files
        .iter()
        .filter_map(|found| {
            let have = words(&query_for(found));
            let matched = wanted
                .iter()
                .filter(|w| have.iter().any(|h| h.starts_with(w.as_str())))
                .count();
            let mut score = if matched == wanted.len() {
                if file_name(found).eq_ignore_ascii_case(name) {
                    1000
                } else {
                    // Fewer extra words: closer.
                    600 - (have.len().saturating_sub(matched) as u32).min(100)
                }
            } else if loose && 2 * matched >= wanted.len() && matched > 0 {
                (300 * matched / wanted.len()) as u32
            } else {
                return None;
            };
            if !folder.is_empty() && folder_name(found).eq_ignore_ascii_case(folder) {
                score += 50;
            }
            Some((score, found))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    scored
        .into_iter()
        .take(MAX_MATCHES)
        .map(|(_, found)| found.clone())
        .collect()
}

/// The audio files in the browser's folders, read in the background.
#[derive(Default)]
struct FileIndex {
    /// The folders read, or being read.
    folders: Vec<PathBuf>,
    files: Vec<PathBuf>,
    scanning: Option<Receiver<Vec<PathBuf>>>,
}

impl FileIndex {
    /// Read `folders` again if they changed; take the files once read.
    fn update(&mut self, folders: &[PathBuf]) {
        if self.folders != folders {
            self.folders = folders.to_vec();
            let (tx, rx) = channel();
            let folders = self.folders.clone();
            std::thread::spawn(move || {
                let _ = tx.send(audio_files(&folders));
            });
            self.scanning = Some(rx);
        }
        if let Some(files) = self.scanning.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.files = files;
            self.scanning = None;
        }
    }

    fn is_scanning(&self) -> bool {
        self.scanning.is_some()
    }

    fn files(&self) -> &[PathBuf] {
        &self.files
    }
}

/// The audio files in `folders` and their subfolders, hidden ones aside.
fn audio_files(folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = folders.to_vec();
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => pending.push(path),
                Ok(_) if is_audio_file(&path) => {
                    files.push(path);
                    if files.len() >= MAX_INDEXED {
                        return files;
                    }
                }
                _ => {}
            }
        }
    }
    files
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_split_into_words_and_numbers() {
        assert_eq!(
            words("FL_140BPM Kit-5 éte"),
            ["fl", "140", "bpm", "kit", "5", "éte"]
        );
    }

    /// The very name first, then names with every word, closest first;
    /// in a folder named alike first among equals.
    #[test]
    fn matches_are_ranked_by_name() {
        let files: Vec<PathBuf> = [
            "/lib/other/kick_soft.wav",
            "/lib/drums/SH_Kick.wav",
            "/lib/kicks/SH_Kick_02.wav",
            "/lib/One Shots/SH_Kick_02.wav",
            "/lib/snare.wav",
        ]
        .map(PathBuf::from)
        .into();
        let missing = Path::new("/gone/One Shots/SH_Kick_02.wav");
        assert_eq!(
            rank(missing, &query_for(missing), &files),
            [
                Path::new("/lib/One Shots/SH_Kick_02.wav"),
                Path::new("/lib/kicks/SH_Kick_02.wav"),
                Path::new("/lib/drums/SH_Kick.wav"),
            ]
        );
        // Typed: every word, as the start of one.
        assert_eq!(
            rank(missing, "kick sof", &files),
            [Path::new("/lib/other/kick_soft.wav")]
        );
        assert!(rank(missing, "", &files).is_empty());
    }
}
