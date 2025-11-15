use std::{
    ffi::OsStr,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

use egui::{
    Color32, Frame, Key, Label, Layout, Margin, Rect, RichText, ScrollArea, Spinner, Ui, Widget,
    pos2,
};

use crate::{
    analysis::AudioInfo,
    cache::AUDIO_ANALYSIS_CACHE,
    core::state::ToniqueProjectState,
    ui::{
        panels::left_panel::DragPayload,
        view::filebrowser::file_tree::{FileNode, FileTree},
        widget::item_button::ItemButton,
    },
};

const PLAYABLE_FORMAT: &[&str] = &["mp3", "wav", "ogg"];

pub struct UIItems {
    selected: Option<usize>,
    pub selected_audio: Option<AudioInfo>,
    files: Arc<Mutex<FileTree>>,
    loading: Arc<Mutex<bool>>,
    query: String,
}

impl UIItems {
    pub fn new() -> Self {
        Self {
            selected: None,
            selected_audio: None,
            files: Arc::new(Mutex::new(FileTree::new())),
            loading: Arc::new(Mutex::new(false)),
            query: "".to_string(),
        }
    }

    pub fn init(&mut self, root: PathBuf) {
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

        let mut next_items = if !is_loading && let Ok(files) = self.files.lock() {
            files.items.clone()
        } else {
            Vec::new()
        };

        if !self.query.is_empty() {
            self.result_ui(ui, next_items.len(), is_loading);
        }
        // If loading dont render the items
        if is_loading {
            return;
        }

        ScrollArea::vertical().show_rows(ui, 16., next_items.len(), |ui, row_range| {
            for index in row_range {
                self.render_item(ui, index, &mut next_items, state);
            }
        });

        if let Ok(mut files) = self.files.lock() {
            files.items = next_items;
        }

        self.update(ui, state);
    }

    pub fn result_ui(&self, ui: &mut Ui, len: usize, is_loading: bool) {
        Frame::new()
            .fill(Color32::from_gray(80))
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
                let is_dir = file.path.is_dir();
                if !is_dir && is_audio {
                    self.selected_audio = AUDIO_ANALYSIS_CACHE.get_or_analyze(file.path.clone());
                    state.play_preview(file.path.clone());
                }
            }
        }
    }

    pub fn render_item(
        &mut self,
        ui: &mut Ui,
        index: usize,
        new_items: &mut Vec<FileNode>,
        state: &mut ToniqueProjectState,
    ) {
        let file = if let Ok(files) = self.files.lock()
            && index < files.items.len()
        {
            files.items[index].clone()
        } else {
            return;
        };

        let is_dir = file.path.is_dir();
        let selected = self.selected.is_some_and(|idx| idx == index);
        let extension = if let Some(ext) = file.path.extension() {
            ext.to_str().unwrap()
        } else {
            ""
        };
        let open = self.files.lock().map_or(false, |f| {
            f.folders.get(&file.path).map_or(false, |f| f.open)
        });

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

        if is_dir
            && pressed
            && let Ok(mut files) = self.files.lock()
        {
            if !open {
                files.open_folder(index, new_items);
            } else {
                files.close_folder(index, new_items);
            }
        }

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
    }

    pub fn clear_search(&mut self, root: PathBuf) {
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
        thread::spawn(move || {
            if let Ok(mut loading) = loading_clone.lock() {
                *loading = true;
            }
            if let Ok(mut files) = files_clone.lock() {
                files.query = query_clone.to_string();
                files.search(root.clone(), &query_clone);

                files.rebuild(root);
            };
            if let Ok(mut loading) = loading_clone.lock() {
                *loading = false;
            }
        });
        self.selected = None;
        self.selected_audio = None;
    }
}
