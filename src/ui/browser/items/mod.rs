mod menus;
mod rows;

use crate::ui::theme::font_size;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
};

use egui::{Key, Rect, RichText, ScrollArea, Stroke, Ui, pos2, vec2};

use crate::{
    analysis::AudioInfo,
    cache::AUDIO_ANALYSIS_CACHE,
    config::keymap::Action,
    core::state::ProjectState,
    ui::{
        browser::file_tree::{FileNode, FileTree, pinned},
        theme::ThemeExt,
    },
};

const PLAYABLE_FORMAT: &[&str] = &["mp3", "wav", "ogg"];
/// Width of a label's menu: its name's field and its colours'.
const LABEL_MENU_WIDTH: f32 = 140.;
/// Most rows pinned at the top while scrolled into folders.
const MAX_PINNED: usize = 4;
/// Height of a row, folders' headers included.
const ROW_HEIGHT: f32 = 16.;

/// What's asked of one of the browser's folders, from its header's menu.
#[derive(Clone, Debug, PartialEq)]
pub enum FolderAction {
    Remove(PathBuf),
    /// Open it in the system's file manager.
    Reveal(PathBuf),
    CollapseAll(PathBuf),
    /// Search in it only.
    SearchIn(PathBuf),
    /// Search in every folder again.
    SearchEverywhere,
    /// Read every folder from disk again.
    Refresh,
    /// Add a folder inside one of them as a folder of its own.
    Add(PathBuf),
    /// Give a file or folder a label, or take its label off.
    SetLabel(PathBuf, Option<usize>),
    RenameLabel(usize, String),
    /// To a colour of [`label_colors::COLORS`].
    RecolorLabel(usize, String),
    /// Take a label off everything.
    ClearLabel(usize),
}

/// A label, as the list shows it.
#[derive(Clone, Debug)]
pub struct LabelView {
    pub name: String,
    pub color: egui::Color32,
}

pub struct FileList {
    /// The browser has the keyboard: Enter opens the selected item.
    focused: bool,
    selected: Option<usize>,
    pub selected_audio: Option<AudioInfo>,
    files: Arc<Mutex<FileTree>>,
    loading: Arc<Mutex<bool>>,
    /// Set when a background search put its results in: the row selected
    /// was one of the rows before.
    landed: Arc<AtomicBool>,
    search_generation: Arc<AtomicU64>,
    query: String,
    /// The folder searched in, or all of them.
    scope: Option<PathBuf>,
    /// Each folder's name as shown: its own, and its parent's when another
    /// folder has the same name.
    names: HashMap<PathBuf, String>,
    /// To redraw once a background search is done.
    ctx: Option<egui::Context>,
    /// Selecting an audio file plays it.
    pub auto_preview: bool,
    labels: Vec<LabelView>,
    /// Each labelled file or folder's label.
    item_labels: HashMap<PathBuf, usize>,
    /// The labels' sections (in the labels' list), by their headers'
    /// paths: their labels, and how many items they have.
    sections: HashMap<PathBuf, (usize, usize)>,
}

impl FileList {
    pub fn new() -> Self {
        Self {
            focused: false,
            selected: None,
            selected_audio: None,
            files: Arc::new(Mutex::new(FileTree::new())),
            loading: Arc::new(Mutex::new(false)),
            landed: Arc::new(AtomicBool::new(false)),
            search_generation: Arc::new(AtomicU64::new(0)),
            query: "".to_string(),
            scope: None,
            names: HashMap::new(),
            ctx: None,
            auto_preview: true,
            labels: Vec::new(),
            item_labels: HashMap::new(),
            sections: HashMap::new(),
        }
    }

    /// The labels, and which file or folder has which.
    pub fn set_labels(&mut self, labels: Vec<LabelView>, item_labels: HashMap<PathBuf, usize>) {
        self.labels = labels;
        self.item_labels = item_labels;
    }

    /// Show a section for each label, with its `items`, all but those in
    /// `collapsed` open.
    pub fn set_sections(&mut self, sections: Vec<(usize, Vec<PathBuf>)>, collapsed: &[PathBuf]) {
        self.search_generation.fetch_add(1, Ordering::Relaxed);
        self.selected = None;
        self.sections = sections
            .iter()
            .map(|(label, items)| (section_path(*label), (*label, items.len())))
            .collect();
        if let Ok(mut files) = self.files.lock() {
            files.set_sections(
                sections
                    .into_iter()
                    .map(|(label, items)| (section_path(label), items))
                    .collect(),
                collapsed,
            );
        }
        if !self.query.is_empty() {
            self.search(&self.query.clone());
        }
    }

    /// Show `roots`, all but `collapsed` open, searching them if a search
    /// is on.
    pub fn set_roots(&mut self, roots: Vec<PathBuf>, collapsed: &[PathBuf]) {
        self.search_generation.fetch_add(1, Ordering::Relaxed);
        self.selected = None;
        self.selected_audio = None;
        self.names = folder_names(&roots);
        if self.scope.as_ref().is_some_and(|s| !roots.contains(s)) {
            self.scope = None;
        }
        if let Ok(mut files) = self.files.lock() {
            files.set_roots(roots, collapsed);
        }
        if !self.query.is_empty() {
            self.search(&self.query.clone());
        }
    }

    /// Read the folders from disk again, keeping what's selected and the
    /// search, if one is on.
    pub fn refresh(&mut self) {
        let Ok(mut files) = self.files.lock() else {
            return;
        };
        let selected = self
            .selected
            .and_then(|i| files.items.get(i))
            .map(|f| f.path.clone());
        files.refresh();
        self.selected = selected.and_then(|path| files.items.iter().position(|f| f.path == path));
        drop(files);
        if !self.query.is_empty() {
            self.search(&self.query.clone());
        }
    }

    /// The roots shown collapsed, unless a search opened them.
    pub fn collapsed_roots(&self) -> Option<Vec<PathBuf>> {
        self.files.lock().ok()?.collapsed_roots()
    }

    pub fn collapse_all(&mut self, root: &Path) {
        if let Ok(mut files) = self.files.lock() {
            files.collapse_all(root);
        }
        self.selected = None;
    }

    /// The name of the folder the search is kept to, if it is.
    pub fn scope_name(&self) -> Option<String> {
        self.names.get(self.scope.as_ref()?).cloned()
    }

    /// A search is running in the background.
    pub fn is_searching(&self) -> bool {
        self.loading.lock().is_ok_and(|l| *l)
    }

    /// Search in `scope` only, or (`None`) in every folder.
    pub fn set_scope(&mut self, scope: Option<PathBuf>) {
        self.scope = scope;
        if !self.query.is_empty() {
            self.search(&self.query.clone());
        }
    }

    /// `focused`: the browser has the keyboard; `moves` are the selection
    /// moves asked for this frame (up or down a row).
    /// Returns what was asked of the folders.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        focused: bool,
        moves: &[Action],
    ) -> Vec<FolderAction> {
        self.focused = focused;
        self.ctx = Some(ui.ctx().clone());
        let mut actions = Vec::new();
        // While a search runs, what was shown stays, and can be selected.
        let item_count = self.files.lock().map_or(0, |files| {
            if self.landed.swap(false, Ordering::Relaxed) {
                self.selected = None;
            }
            files.items.len()
        });
        if !self.query.is_empty() && item_count == 0 && !self.is_searching() {
            ui.add_space(8.);
            let scope = self.scope_name();
            ui.vertical_centered(|ui| {
                let text = match &scope {
                    Some(folder) => format!("No files match in {folder}"),
                    None => "No files match".into(),
                };
                ui.label(
                    RichText::new(text)
                        .size(font_size::LABEL)
                        .color(ui.app_theme().text_muted),
                );
                if scope.is_some() && ui.link("Search all folders").clicked() {
                    actions.push(FolderAction::SearchEverywhere);
                }
            });
            return actions;
        }

        let mut folder_toggles = Vec::new();
        let files_arc = self.files.clone();
        ScrollArea::vertical().show_rows(ui, ROW_HEIGHT, item_count, |ui, row_range| {
            let visible_rows = files_arc
                .lock()
                .map(|files| {
                    row_range
                        .clone()
                        .filter_map(|index| {
                            files.items.get(index).map(|file| {
                                let open = files
                                    .folders
                                    .get(&file.path)
                                    .is_some_and(|folder| folder.open);
                                (index, file.clone(), open)
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            // Scrolled into folders: the way down to the first row shown (a
            // folder's header, the subfolders it's in) stays at the top.
            let view_top = ui.clip_rect().top();
            let pinned: Vec<(usize, FileNode, bool)> = files_arc
                .lock()
                .map(|files| {
                    pinned(&files.items, row_range.start, MAX_PINNED)
                        .into_iter()
                        .map(|i| {
                            let file = files.items[i].clone();
                            let open = files.folders.get(&file.path).is_some_and(|f| f.open);
                            (i, file, open)
                        })
                        .collect()
                })
                .unwrap_or_default();
            // Where each row is: a pinned folder is pushed up by the first
            // row after its contents.
            let mut tops = HashMap::new();
            for (index, file, open) in visible_rows {
                tops.insert(index, ui.next_widget_position().y);
                let toggled = if file.depth == 0 {
                    self.root_ui(ui, index, &file, open, &mut actions)
                } else {
                    self.item_ui(ui, index, &file, open, state, &mut actions, false)
                };
                if toggled {
                    folder_toggles.push(index);
                }
            }
            let ends: Vec<Option<f32>> = files_arc
                .lock()
                .map(|files| {
                    pinned
                        .iter()
                        .map(|(i, file, _)| {
                            let end = (i + 1..files.items.len())
                                .find(|&j| files.items[j].depth <= file.depth)?;
                            tops.get(&end).copied()
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut y: Vec<f32> = Vec::with_capacity(pinned.len());
            for (level, end) in ends.iter().enumerate() {
                let slot = view_top + level as f32 * ROW_HEIGHT;
                let pushed = end.map_or(slot, |end| slot.min(end - ROW_HEIGHT));
                // Under its parent, moved up with it.
                let under = level
                    .checked_sub(1)
                    .map_or(pushed, |parent| pushed.min(y[parent] + ROW_HEIGHT));
                y.push(under);
            }
            // Deepest first: a pushed up subfolder slides under its parent.
            for (level, (index, file, open)) in pinned.iter().enumerate().rev() {
                let rect = Rect::from_min_size(
                    pos2(ui.max_rect().left(), y[level]),
                    vec2(ui.max_rect().width(), ROW_HEIGHT),
                );
                let theme = ui.app_theme();
                ui.painter().rect_filled(rect, 0., theme.bg_panel);
                if level + 1 == pinned.len() {
                    ui.painter().hline(
                        rect.x_range(),
                        rect.bottom() - 0.5,
                        Stroke::new(1., theme.separator),
                    );
                }
                let toggled = ui
                    .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                        if file.depth == 0 {
                            self.root_ui(ui, *index, file, *open, &mut actions)
                        } else {
                            self.item_ui(ui, *index, file, *open, state, &mut actions, true)
                        }
                    })
                    .inner;
                if toggled {
                    folder_toggles.push(*index);
                }
            }
        });

        // A folder both in view and pinned is drawn twice: Enter reaches
        // both, but opens or closes it once.
        folder_toggles.sort_unstable();
        folder_toggles.dedup();
        if let Ok(mut files) = self.files.lock() {
            for index in folder_toggles {
                files.toggle_folder(index);
            }
        }

        for action in moves {
            self.move_selection(state, *action == Action::MoveTrackUp);
        }
        if let Some(action) = self.label_keys(ui) {
            actions.push(action);
        }
        actions
    }

    /// With the keyboard, and a row selected: 1 to 7 give it that label,
    /// 0 takes its label off. Not a label's header.
    fn label_keys(&self, ui: &Ui) -> Option<FolderAction> {
        if !self.focused || ui.ctx().egui_wants_keyboard_input() {
            return None;
        }
        let path = self
            .files
            .lock()
            .ok()?
            .items
            .get(self.selected?)?
            .path
            .clone();
        if self.sections.contains_key(&path) {
            return None;
        }
        const KEYS: [Key; 8] = [
            Key::Num0,
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
        ];
        let pressed = ui.input_mut(|i| {
            KEYS.iter()
                .position(|&key| i.consume_key(egui::Modifiers::NONE, key))
        })?;
        let label = pressed.checked_sub(1).filter(|&l| l < self.labels.len());
        if pressed > 0 && label.is_none() {
            return None;
        }
        // Its own label's key again: taken off.
        let label = label.filter(|&l| self.item_labels.get(&path) != Some(&l));
        Some(FolderAction::SetLabel(path, label))
    }

    /// The file at `path` was selected: play it if previews play on their
    /// own; otherwise stop what played, it being another file's.
    fn preview(&self, state: &mut ProjectState, path: PathBuf) {
        if self.auto_preview {
            state.play_preview(path);
        } else if state.previewing().is_some() {
            state.pause_preview();
        }
    }

    /// Move the selection a row up or down, previewing the file selected.
    fn move_selection(&mut self, state: &mut ProjectState, up: bool) {
        if let Some(index) = self.selected.as_mut() {
            let mut updated = false;
            if up && *index > 0 {
                *index -= 1;
                updated = true;
            } else if !up && *index + 1 < self.files.lock().map_or(0, |f| f.items.len()) {
                *index += 1;
                updated = true;
            }
            let Some(file) = self
                .files
                .lock()
                .ok()
                .and_then(|files| files.items.get(*index).cloned())
            else {
                return;
            };

            let extension = file.path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if updated {
                let is_audio = PLAYABLE_FORMAT.contains(&extension);
                if !file.is_dir && is_audio {
                    self.selected_audio = AUDIO_ANALYSIS_CACHE.get_or_analyze(file.path.clone());
                    self.preview(state, file.path.clone());
                }
            }
        }
    }

    pub fn clear_search(&mut self) {
        self.search_generation.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut loading) = self.loading.lock() {
            *loading = false;
        }
        self.query = "".to_string();
        if let Ok(mut files) = self.files.lock() {
            files.clear_search();
        }
        self.selected = None;
        self.selected_audio = None;
    }

    /// Search the folders (or the one searched in) in the background.
    pub fn search(&mut self, query: &str) {
        self.query = query.to_string();
        let scope = self.scope.clone();
        let query_clone = query.to_string();
        let files_clone = self.files.clone();
        let loading_clone = self.loading.clone();
        let landed = self.landed.clone();
        let generation = self.search_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let generation_clone = self.search_generation.clone();
        if let Ok(mut loading) = self.loading.lock() {
            *loading = true;
        }
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            // Read from disk on a copy, the tree staying free to draw; only
            // the results are put in it.
            let Some(mut search) = files_clone.lock().ok().map(|files| files.start_search()) else {
                return;
            };
            search.run(&query_clone, scope.as_deref());
            // Under the lock, so a search started or cleared meanwhile wins.
            let Ok(mut files) = files_clone.lock() else {
                return;
            };
            if generation_clone.load(Ordering::Relaxed) != generation {
                return;
            }
            files.finish_search(&query_clone, search);
            landed.store(true, Ordering::Relaxed);
            drop(files);
            if let Ok(mut loading) = loading_clone.lock() {
                *loading = false;
            }
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
        self.selected = None;
        self.selected_audio = None;
    }
}

/// Each folder's name as shown: its own, or with its parent's after it
/// (`Samples — Splice`) when another has the same name.
fn folder_names(roots: &[PathBuf]) -> HashMap<PathBuf, String> {
    let name = |path: &Path| {
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string())
    };
    roots
        .iter()
        .map(|root| {
            let own = name(root);
            let shared = roots.iter().filter(|other| name(other) == own).count() > 1;
            let shown = match root.parent().filter(|_| shared) {
                Some(parent) => format!("{own} — {}", name(parent)),
                None => own,
            };
            (root.clone(), shown)
        })
        .collect()
}

/// The path naming label `index`'s section in the labels' list: no file's.
pub fn section_path(index: usize) -> PathBuf {
    PathBuf::from(format!("\u{0}label-{index}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_of_the_same_name_tell_their_parents() {
        let roots = [
            PathBuf::from("/a/Splice/Samples"),
            PathBuf::from("/b/Drums/Samples"),
            PathBuf::from("/c/Loops"),
        ];
        let names = folder_names(&roots);
        assert_eq!(names[&roots[0]], "Samples — Splice");
        assert_eq!(names[&roots[1]], "Samples — Drums");
        assert_eq!(names[&roots[2]], "Loops");
    }
}
