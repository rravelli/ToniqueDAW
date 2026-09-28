use std::{
    ffi::OsStr,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};

use egui::{
    Frame, Key, Label, Layout, Margin, Rect, RichText, ScrollArea, Spinner, Ui, Widget, pos2,
};

use crate::{
    analysis::AudioInfo,
    cache::AUDIO_ANALYSIS_CACHE,
    core::state::ToniqueProjectState,
    ui::{
        panels::left_panel::DragPayload,
        theme::ThemeExt,
        view::filebrowser::file_tree::{FileNode, FileTree},
        widget::item_button::ItemButton,
    },
};

const PLAYABLE_FORMAT: &[&str] = &["mp3", "wav", "ogg"];

pub struct FileList {
    selected: Option<usize>,
    pub selected_audio: Option<AudioInfo>,
    files: Arc<Mutex<FileTree>>,
    loading: Arc<Mutex<bool>>,
    search_generation: Arc<AtomicU64>,
    query: String,
}

impl FileList {
    pub fn new() -> Self {
        Self {
            selected: None,
            selected_audio: None,
            files: Arc::new(Mutex::new(FileTree::new())),
            loading: Arc::new(Mutex::new(false)),
            search_generation: Arc::new(AtomicU64::new(0)),
            query: "".to_string(),
        }
    }

    pub fn init(&mut self, root: PathBuf) {
        self.search_generation.fetch_add(1, Ordering::Relaxed);
        self.selected = None;
        self.selected_audio = None;
        if let Ok(mut files) = self.files.lock() {
            files.init(root.clone());
        }
        if !self.query.is_empty() {
            self.search(&self.query.clone(), root);
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        let is_loading = self.loading.lock().is_ok_and(|l| *l);
        let item_count = if !is_loading {
            self.files.lock().map_or(0, |files| files.items.len())
        } else {
            0
        };

        if !self.query.is_empty() {
            self.result_ui(ui, item_count, is_loading);
        }
        if is_loading {
            return;
        }

        let mut folder_toggles = Vec::new();
        let files_arc = self.files.clone();
        ScrollArea::vertical().show_rows(ui, 16., item_count, |ui, row_range| {
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

            for (index, file, open) in visible_rows {
                if self.item_ui(ui, index, &file, open, state) {
                    folder_toggles.push(index);
                }
            }
        });

        if let Ok(mut files) = self.files.lock() {
            for index in folder_toggles {
                files.toggle_folder(index);
            }
        }

        self.update(ui, state);
    }

    pub fn result_ui(&self, ui: &mut Ui, len: usize, is_loading: bool) {
        Frame::new()
            .fill(ui.app_theme().bg_control)
            .inner_margin(Margin::symmetric(4, 2))
            .corner_radius(2.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let text = if len == 0 && !is_loading {
                        "No result found".to_string()
                    } else {
                        format!("Searching '{}'", self.query)
                    };

                    Label::new(RichText::new(text).size(10.))
                        .selectable(false)
                        .truncate()
                        .ui(ui);

                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if is_loading {
                            ui.add(Spinner::new().size(14.));
                        }
                    });
                });
            });
        ui.add_space(5.0);
    }

    pub fn update(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        if let Some(index) = self.selected.as_mut() {
            let mut updated = false;
            if ui.input(|i| i.key_pressed(Key::ArrowUp)) && *index > 0 {
                *index -= 1;
                updated = true;
            } else if ui.input(|i| i.key_pressed(Key::ArrowDown))
                && *index < self.files.lock().map_or(0, |f| f.items.len() - 1)
            {
                *index += 1;
                updated = true;
            }
            let file = if let Ok(files) = self.files.lock() {
                files.items[*index].clone()
            } else {
                return;
            };

            let extension = if let Some(ext) = file.path.extension() {
                ext.to_str().unwrap()
            } else {
                ""
            };
            if updated {
                let is_audio = PLAYABLE_FORMAT.contains(&extension);
                if !file.is_dir && is_audio {
                    self.selected_audio = AUDIO_ANALYSIS_CACHE.get_or_analyze(file.path.clone());
                    state.play_preview(file.path.clone());
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
        state: &mut ToniqueProjectState,
    ) -> bool {
        let is_dir = file.is_dir;
        let selected = self.selected.is_some_and(|idx| idx == index);
        let extension = if let Some(ext) = file.path.extension() {
            ext.to_str().unwrap()
        } else {
            ""
        };
        let is_audio = PLAYABLE_FORMAT.contains(&extension);

        let icon = if is_dir {
            if open {
                egui_phosphor::fill::CARET_DOWN
            } else {
                egui_phosphor::fill::CARET_RIGHT
            }
        } else {
            if is_audio {
                egui_phosphor::fill::FILE_AUDIO
            } else {
                egui_phosphor::fill::FILE
            }
        };

        let name = file
            .path
            .file_name()
            .unwrap_or(OsStr::new(""))
            .to_string_lossy();

        let res = ui.add(
            ItemButton::new(format!(
                "{}{} {}",
                " ".repeat((file.depth - 1) * 2),
                icon,
                name
            ))
            .selected(selected),
        );

        let pressed = res.clicked() || (selected && ui.input(|i| i.key_pressed(Key::Enter)));

        if pressed {
            self.selected = Some(index);
        }

        if is_audio && pressed {
            self.selected_audio = AUDIO_ANALYSIS_CACHE.get_or_analyze(file.path.clone());
            state.play_preview(file.path.clone());
        }

        let toggle_folder = is_dir && pressed;

        if res.dragged() {
            if is_audio
                && let Some(audio_info) = AUDIO_ANALYSIS_CACHE.get_or_analyze(file.path.clone())
            {
                res.dnd_set_drag_payload(DragPayload::File(audio_info));
            }
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

    pub fn clear_search(&mut self, root: PathBuf) {
        self.search_generation.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut loading) = self.loading.lock() {
            *loading = false;
        }
        self.query = "".to_string();
        if let Ok(mut files) = self.files.lock() {
            files.query.clear();
            files.rebuild(root);
        }
        self.selected = None;
        self.selected_audio = None;
    }

    pub fn search(&mut self, query: &str, root: PathBuf) {
        self.query = query.to_string();
        let query_clone = query.to_string();
        let files_clone = self.files.clone();
        let loading_clone = self.loading.clone();
        let generation = self.search_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let generation_clone = self.search_generation.clone();
        if let Ok(mut loading) = self.loading.lock() {
            *loading = true;
        }
        thread::spawn(move || {
            if generation_clone.load(Ordering::Relaxed) != generation {
                return;
            }
            if let Ok(mut files) = files_clone.lock() {
                if generation_clone.load(Ordering::Relaxed) != generation {
                    return;
                }
                files.query = query_clone.clone();
                files.search(root.clone(), &query_clone);
                if generation_clone.load(Ordering::Relaxed) == generation {
                    files.rebuild(root);
                }
            };
            if generation_clone.load(Ordering::Relaxed) == generation {
                if let Ok(mut loading) = loading_clone.lock() {
                    *loading = false;
                }
            }
        });
        self.selected = None;
        self.selected_audio = None;
    }
}
