use std::{ffi::OsStr, fs, path::PathBuf};

use eframe::egui;
use egui::{Sense, TextEdit};
use rfd::FileDialog;

use crate::{
    analysis::AudioInfo,
    cache::AUDIO_ANALYSIS_CACHE,
    components::buttons::left_aligned_selectable,
    config::{load_work_dir, save_work_dir},
};

#[derive(Clone)]
pub struct File {
    pub entry: PathBuf,
    pub open: bool,
    children: Option<Vec<File>>,
}

fn is_audio_file(path: &PathBuf) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "mp3" | "wav" | "flac" | "ogg" | "aiff" | "aac" | "m4a" | "midi" | "mid"
            )
        })
        .unwrap_or(false)
}

impl File {
    fn get_children(&mut self) -> &mut Vec<File> {
        if self.children.is_none() {
            self.children = Some(Self::read_entries(self.entry.clone()));
        }
        self.children.as_mut().unwrap()
    }

    fn read_entries(dir: PathBuf) -> Vec<File> {
        let mut entries = fs::read_dir(dir)
            .map(|read_dir| {
                read_dir
                    .filter_map(|entry| entry.ok())
                    .map(|f| f.path())
                    .filter(|path| path.is_dir() || is_audio_file(path))
                    .map(|entry| File {
                        children: None,
                        entry: entry,
                        open: false,
                    })
                    .collect()
            })
            .unwrap_or_else(|_| vec![]);

        entries.sort_by_key(|file| {
            file.entry
                .file_name()
                .map(|s| s.to_os_string())
                .unwrap_or_default()
        });

        entries
    }
}

pub fn filter_files(file: &mut File, query: &str) -> Option<File> {
    let name_matches = file
        .entry
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_lowercase().contains(&query.to_lowercase()))
        .unwrap_or(false);

    if file.children.is_none() {
        file.get_children();
    }

    let filtered_children = file.children.as_mut().and_then(|children| {
        let matching_children: Vec<File> = children
            .iter_mut()
            .filter_map(|child| filter_files(child, query))
            .collect();

        if matching_children.is_empty() {
            None
        } else {
            Some(matching_children)
        }
    });

    if name_matches || filtered_children.is_some() {
        Some(File {
            entry: file.entry.clone(),
            open: file.open,
            children: filtered_children,
        })
    } else {
        None
    }
}

pub struct FilePicker {
    dir: Vec<File>,
    search: Option<File>,
    search_query: String,

    // Layout
    width: f32,

    is_released: bool,
}

impl FilePicker {
    pub fn new() -> Self {
        let mut dir = vec![];
        if let Some(path) = load_work_dir() {
            let mut file = File {
                children: None,
                entry: PathBuf::from(path),
                open: true,
            };
            file.get_children();
            dir.push(file);
        }

        Self {
            dir,
            width: 200.,
            search: None,
            is_released: false,
            search_query: "".to_string(),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> (Option<AudioInfo>, bool) {
        let mut dragged_audio_info = None;

        ui.vertical(|ui| {
            ui.set_width(self.width);
            if ui.button("Select folder").clicked() {
                let picked_dir = FileDialog::new().pick_folder();
                if let Some(path) = picked_dir {
                    save_work_dir(&path.to_str().unwrap());
                    let mut file = File {
                        children: None,
                        entry: path,
                        open: true,
                    };
                    file.get_children();
                    self.dir = vec![file];
                }
            };
            // ui.horizontal(|ui| {
            //     let res = ui
            //         .add(TextEdit::singleline(&mut self.search_query).hint_text("Seach (ctrl+F)"));

            //     if res.changed() {
            //         let mut clone = self.dir.clone();
            //         self.search = filter_files(&mut clone, &self.search_query);
            //     }
            // });

            egui::ScrollArea::vertical().show(ui, |ui| {
                if !self.search_query.is_empty()
                    && let Some(file) = &mut self.search
                {
                    let (audio_info, is_released) = Self::show_buttons(ui, file, 0);
                    dragged_audio_info = audio_info;
                    self.is_released = is_released;
                } else {
                    for dir in self.dir.iter_mut() {
                        let (audio_info, is_released) = Self::show_buttons(ui, dir, 0);
                        dragged_audio_info = audio_info;
                        self.is_released = is_released;
                    }
                }
            });
        });
        self.resize_handle(ui);

        (dragged_audio_info, self.is_released)
    }

    pub fn show_buttons(
        ui: &mut egui::Ui,
        f: &mut File,
        depth: usize,
    ) -> (Option<AudioInfo>, bool) {
        let is_dir = f.entry.is_dir();
        let mut audio_info = None;
        let mut is_released = false;

        let extension = if let Some(ext) = &f.entry.extension() {
            ext.to_str().unwrap()
        } else {
            ""
        };

        let is_audio = ["mp3", "wav"].contains(&extension);

        let icon = if is_dir {
            if f.open {
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

        let clone = f.entry.clone();
        let name = clone.file_name().unwrap().to_string_lossy();

        let label_response =
            left_aligned_selectable(ui, format!("{}{} {}", " ".repeat(depth * 2), icon, name));

        // On click
        if label_response.clicked() {
            if is_dir {
                f.open = !f.open;
            } else if is_audio {
                AUDIO_ANALYSIS_CACHE.get_or_analyze(f.entry.clone());
            }
        };

        // On drag started
        if label_response.drag_started() {}

        // On drag
        if label_response.dragged() && !is_dir && is_audio {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::Grabbing);
            audio_info = AUDIO_ANALYSIS_CACHE.get_or_analyze(f.entry.clone());
        }

        if label_response.drag_stopped() {
            is_released = true;
        }

        if is_dir && f.open {
            let children = f.get_children();

            for child in children {
                let (child_audio_info, released) = Self::show_buttons(ui, child, depth + 1);
                if child_audio_info.is_some() {
                    audio_info = child_audio_info;
                }
                is_released = is_released || released;
            }
        }

        (audio_info, is_released)
    }

    fn resize_handle(&mut self, ui: &mut egui::Ui) {
        let (response, painter) =
            ui.allocate_painter(egui::Vec2::new(6., ui.available_height()), Sense::drag());

        painter.rect_filled(response.rect, 0., egui::Color32::from_gray(40));

        if response.hovered() {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
        }

        if response.dragged() {
            self.width += response.drag_delta().x;
            self.width = self.width.clamp(40., 500.);
        }
    }
}
