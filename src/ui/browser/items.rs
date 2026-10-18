use crate::ui::theme::font_size;
use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};

use egui::{Align2, FontId, Key, Popup, Rect, RichText, ScrollArea, Sense, Stroke, Ui, pos2, vec2};
use egui_phosphor::regular::{
    ARROW_SQUARE_OUT, ARROWS_IN_LINE_VERTICAL, DOTS_THREE, MAGNIFYING_GLASS, X,
};

use crate::{
    analysis::AudioInfo,
    cache::AUDIO_ANALYSIS_CACHE,
    config::keymap::Action,
    core::state::ProjectState,
    ui::browser::preview::details,
    ui::{
        browser::file_tree::{FileNode, FileTree},
        dnd::DragPayload,
        font::PHOSPHOR_REGULAR,
        theme::{ThemeExt, with_alpha},
        widget::{
            context_menu::ContextMenuButton,
            flat_button::FlatButton,
            list_row::{ListRow, SLOT},
        },
    },
};

const PLAYABLE_FORMAT: &[&str] = &["mp3", "wav", "ogg"];
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
    /// Add a folder inside one of them as a folder of its own.
    Add(PathBuf),
}

pub struct FileList {
    /// The browser has the keyboard: Enter opens the selected item.
    focused: bool,
    selected: Option<usize>,
    pub selected_audio: Option<AudioInfo>,
    files: Arc<Mutex<FileTree>>,
    loading: Arc<Mutex<bool>>,
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
}

impl FileList {
    pub fn new() -> Self {
        Self {
            focused: false,
            selected: None,
            selected_audio: None,
            files: Arc::new(Mutex::new(FileTree::new())),
            loading: Arc::new(Mutex::new(false)),
            search_generation: Arc::new(AtomicU64::new(0)),
            query: "".to_string(),
            scope: None,
            names: HashMap::new(),
            ctx: None,
            auto_preview: true,
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
        // While a search runs, what was shown stays.
        let item_count = self.files.lock().map_or(0, |files| files.items.len());
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

            // Scrolled into a folder: its header stays at the top (pushed up
            // by the next one's as it comes), in place of its own row.
            let view_top = ui.clip_rect().top();
            let first_top = ui.next_widget_position().y;
            let sticky = files_arc.lock().ok().and_then(|files| {
                let index = files.items[..=row_range.start.min(files.items.len().checked_sub(1)?)]
                    .iter()
                    .rposition(|f| f.depth == 0)?;
                let hidden = index < row_range.start || first_top < view_top;
                let open = files
                    .folders
                    .get(&files.items[index].path)
                    .is_some_and(|f| f.open);
                hidden.then(|| (index, files.items[index].clone(), open))
            });
            let mut next_root_top = None;
            for (index, file, open) in visible_rows {
                let toggled = if sticky.as_ref().is_some_and(|(i, ..)| *i == index) {
                    ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
                    false
                } else if file.depth == 0 {
                    next_root_top.get_or_insert(ui.next_widget_position().y);
                    self.root_ui(ui, index, &file, open, &mut actions)
                } else {
                    self.item_ui(ui, index, &file, open, state, &mut actions)
                };
                if toggled {
                    folder_toggles.push(index);
                }
            }
            if let Some((index, root, open)) = sticky {
                let top = next_root_top.map_or(view_top, |next| view_top.min(next - ROW_HEIGHT));
                let rect = Rect::from_min_size(
                    pos2(ui.max_rect().left(), top),
                    vec2(ui.max_rect().width(), ROW_HEIGHT),
                );
                ui.painter().rect_filled(rect, 0., ui.app_theme().bg_panel);
                let toggled = ui
                    .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                        self.root_ui(ui, index, &root, open, &mut actions)
                    })
                    .inner;
                if toggled {
                    folder_toggles.push(index);
                }
            }
        });

        if let Ok(mut files) = self.files.lock() {
            for index in folder_toggles {
                files.toggle_folder(index);
            }
        }

        for action in moves {
            self.move_selection(state, *action == Action::MoveTrackUp);
        }
        actions
    }

    /// One of the browser's folders: a header, its name in small capitals
    /// (its path on hover), with a menu of what can be done with it on its
    /// right while hovered, or on a right-click. Returns whether it was
    /// clicked, to open or close.
    fn root_ui(
        &mut self,
        ui: &mut Ui,
        index: usize,
        file: &FileNode,
        open: bool,
        actions: &mut Vec<FolderAction>,
    ) -> bool {
        let theme = ui.app_theme();
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
        let selected = self.selected == Some(index);
        let painter = ui.painter_at(rect);
        if selected {
            painter.rect_filled(rect, 0., with_alpha(theme.accent, 60));
        } else if response.hovered() {
            painter.rect_filled(rect, 0., theme.hover_overlay);
        }
        // Between folders.
        if index > 0 {
            painter.hline(
                rect.x_range(),
                rect.top() + 0.5,
                Stroke::new(1., theme.separator),
            );
        }
        let caret = if open {
            egui_phosphor::regular::CARET_DOWN
        } else {
            egui_phosphor::regular::CARET_RIGHT
        };
        let x = rect.left() + 6.;
        painter.text(
            pos2(x + SLOT / 2., rect.center().y),
            Align2::CENTER_CENTER,
            caret,
            FontId::proportional(12.),
            theme.text_muted,
        );
        let name = self
            .names
            .get(&file.path)
            .cloned()
            .unwrap_or_default()
            .to_uppercase();
        let menu_width = ROW_HEIGHT;
        // The folder searched in alone: a magnifier before its name, and a
        // button to search them all again before its menu.
        let scoped = self.scope.as_ref() == Some(&file.path);
        let mut name_x = x + SLOT;
        if scoped {
            painter.text(
                pos2(name_x + SLOT / 2., rect.center().y),
                Align2::CENTER_CENTER,
                MAGNIFYING_GLASS,
                FontId::proportional(11.),
                theme.accent,
            );
            name_x += SLOT;
        }
        let buttons = if scoped { 2. } else { 1. } * menu_width;
        let galley = egui::WidgetText::from(RichText::new(name).strong()).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            (rect.right() - name_x - buttons - 4.).max(0.),
            FontId::proportional(font_size::SMALL),
        );
        painter.galley(
            pos2(name_x, rect.center().y - galley.size().y / 2.),
            galley,
            if scoped { theme.text } else { theme.text_muted },
        );

        let path = file.path.clone();
        let menu = |ui: &mut Ui, actions: &mut Vec<FolderAction>| {
            let search = if scoped {
                ("Search all folders", FolderAction::SearchEverywhere)
            } else {
                (
                    "Search in this folder",
                    FolderAction::SearchIn(path.clone()),
                )
            };
            for (icon, text, action) in [
                (MAGNIFYING_GLASS, search.0, search.1),
                (
                    ARROWS_IN_LINE_VERTICAL,
                    "Collapse all",
                    FolderAction::CollapseAll(path.clone()),
                ),
                (
                    ARROW_SQUARE_OUT,
                    "Show in file manager",
                    FolderAction::Reveal(path.clone()),
                ),
            ] {
                if ui.add(ContextMenuButton::new(icon, text)).clicked() {
                    actions.push(action);
                }
            }
            ui.separator();
            if ui
                .add(
                    ContextMenuButton::new(
                        egui_phosphor::regular::FOLDER_MINUS,
                        "Remove from browser",
                    )
                    .text_color(ui.app_theme().danger),
                )
                .clicked()
            {
                actions.push(FolderAction::Remove(path.clone()));
            }
        };
        // The menu's button, while the header's hovered or its menu open.
        let button_rect = Rect::from_min_size(
            pos2(rect.right() - menu_width - 2., rect.top()),
            vec2(menu_width, ROW_HEIGHT),
        );
        let menu_id = ui.id().with(("folder-menu", &file.path));
        let menu_open = Popup::is_id_open(ui.ctx(), menu_id);
        let mut over_button = false;
        if response.hovered() || menu_open || ui.rect_contains_pointer(button_rect) {
            let button = ui.put(
                button_rect,
                FlatButton::ghost(DOTS_THREE)
                    .square(menu_width)
                    .font(FontId::new(
                        12.,
                        egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                    )),
            );
            over_button = button.hovered();
            Popup::menu(&button)
                .id(menu_id)
                .show(|ui| menu(ui, actions));
        }
        if scoped {
            let clear = ui.put(
                button_rect.translate(vec2(-menu_width, 0.)),
                FlatButton::ghost(X)
                    .square(menu_width)
                    .font(FontId::new(
                        11.,
                        egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                    ))
                    .tooltip("Search all folders"),
            );
            over_button |= clear.hovered();
            if clear.clicked() {
                actions.push(FolderAction::SearchEverywhere);
            }
        }
        response.context_menu(|ui| menu(ui, actions));
        let response = response.on_hover_text(file.path.display().to_string());
        let pressed = !over_button
            && (response.clicked()
                || (selected && self.focused && ui.input(|i| i.key_pressed(Key::Enter))));
        if pressed {
            self.selected = Some(index);
        }
        pressed
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
            } else if !up && *index < self.files.lock().map_or(0, |f| f.items.len() - 1) {
                *index += 1;
                updated = true;
            }
            let file = if let Ok(files) = self.files.lock() {
                files.items[*index].clone()
            } else {
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

    pub fn item_ui(
        &mut self,
        ui: &mut Ui,
        index: usize,
        file: &FileNode,
        open: bool,
        state: &mut ProjectState,
        actions: &mut Vec<FolderAction>,
    ) -> bool {
        let is_dir = file.is_dir;
        let selected = self.selected.is_some_and(|idx| idx == index);
        let extension = file.path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let is_audio = PLAYABLE_FORMAT.contains(&extension);

        let (caret, icon) = match (is_dir, open) {
            (true, true) => (
                Some(egui_phosphor::regular::CARET_DOWN),
                egui_phosphor::regular::FOLDER_OPEN,
            ),
            (true, false) => (
                Some(egui_phosphor::regular::CARET_RIGHT),
                egui_phosphor::regular::FOLDER,
            ),
            (false, _) if is_audio => (None, egui_phosphor::regular::FILE_AUDIO),
            (false, _) => (None, egui_phosphor::regular::FILE),
        };
        // The file the preview plays: a speaker, in the accent.
        let playing = state.previewing() == Some(file.path.as_path());
        let icon = if playing {
            egui_phosphor::regular::SPEAKER_HIGH
        } else {
            icon
        };

        let name = file
            .path
            .file_name()
            .unwrap_or(OsStr::new(""))
            .to_string_lossy();

        let mut row = ListRow::new(name)
            .indent((file.depth - 1) as f32 * SLOT)
            .guides(file.depth - 1)
            .caret(caret)
            .icon(icon)
            .selected(selected);
        if playing {
            row = row.icon_color(ui.app_theme().accent);
        }
        let res = ui.add(row);
        // Details of an audio file on hover; what a folder can do on a
        // right-click.
        let res = if is_audio {
            res.on_hover_ui(|ui| {
                if let Some(audio) = AUDIO_ANALYSIS_CACHE.get_or_analyze(file.path.clone()) {
                    ui.label(RichText::new(details(&audio)).size(font_size::SMALL));
                }
            })
        } else {
            res
        };
        if is_dir {
            res.context_menu(|ui| {
                if ui
                    .add(ContextMenuButton::new(
                        egui_phosphor::regular::FOLDER_PLUS,
                        "Add as browser folder",
                    ))
                    .clicked()
                {
                    actions.push(FolderAction::Add(file.path.clone()));
                }
                if ui
                    .add(ContextMenuButton::new(
                        ARROW_SQUARE_OUT,
                        "Show in file manager",
                    ))
                    .clicked()
                {
                    actions.push(FolderAction::Reveal(file.path.clone()));
                }
            });
        }

        let pressed =
            res.clicked() || (selected && self.focused && ui.input(|i| i.key_pressed(Key::Enter)));

        if pressed {
            self.selected = Some(index);
        }

        if is_audio && pressed {
            self.selected_audio = AUDIO_ANALYSIS_CACHE.get_or_analyze(file.path.clone());
            self.preview(state, file.path.clone());
        }

        let toggle_folder = is_dir && pressed;

        if res.dragged()
            && is_audio
            && let Some(audio_info) = AUDIO_ANALYSIS_CACHE.get_or_analyze(file.path.clone())
        {
            // Drawn on the timeline while dragged.
            audio_info.load_peaks();
            res.dnd_set_drag_payload(DragPayload::File(audio_info));
        }

        if selected {
            ui.scroll_to_rect(
                Rect::from_min_max(
                    pos2(res.rect.left(), res.rect.top() - 20.),
                    pos2(res.rect.right(), res.rect.bottom() + 20.),
                ),
                None,
            );
        }
        toggle_folder
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
