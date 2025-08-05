use std::{ffi::OsStr, fs, path::PathBuf};

use eframe::egui;
use egui::{Color32, Pos2, Sense, Shape, Stroke, TextEdit, Ui, Vec2};
use rfd::FileDialog;
use rtrb::Producer;

use crate::{
    analysis::AudioInfo,
    cache::AUDIO_ANALYSIS_CACHE,
    components::{
        buttons::left_aligned_selectable, waveform::UIWaveform, workspace::PlaybackState,
    },
    config::{load_work_dir, save_work_dir},
    message::GuiToPlayerMsg,
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
    waveform: UIWaveform,
    selected: Option<AudioInfo>,
    pub preview_position: usize,
    pub preview_state: PlaybackState,
    // Layout
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

            search: None,
            is_released: false,
            search_query: "".to_string(),
            waveform: UIWaveform::new(),
            selected: None,
            preview_position: 0,
            preview_state: PlaybackState::Paused,
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) -> (Option<AudioInfo>, bool) {
        let mut dragged_audio_info = None;

        if self.preview_state == PlaybackState::Playing {
            ui.ctx().request_repaint();
        }
        ui.set_min_size(ui.available_size());
        ui.vertical(|ui| {
            ui.set_height(ui.available_height());
            // ui.set_width(self.width);
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

            let mut new_selected = None;
            egui::ScrollArea::vertical()
                .max_height(ui.available_height() - 50.)
                .show(ui, |ui| {
                    ui.set_min_height(ui.available_height());
                    if !self.search_query.is_empty()
                        && let Some(file) = &mut self.search
                    {
                        let (audio_info, is_released, selected_info) =
                            Self::show_buttons(ui, file, 0, self.selected.clone().map(|f| f.path));
                        dragged_audio_info = audio_info;
                        self.is_released = is_released;
                        new_selected = selected_info;
                    } else {
                        for dir in self.dir.iter_mut() {
                            let (audio_info, is_released, selected_info) = Self::show_buttons(
                                ui,
                                dir,
                                0,
                                self.selected.clone().map(|f| f.path),
                            );
                            dragged_audio_info = audio_info;
                            self.is_released = is_released;
                            new_selected = selected_info;
                        }
                    }
                });

            if let Some(info) = new_selected {
                let _ = tx.push(GuiToPlayerMsg::PlayPreview(info.path.clone()));
                self.preview_state = PlaybackState::Playing;
                self.preview_position = 0;
                self.selected = Some(info);
            }
            self.preview(ui, tx);
        });

        (dragged_audio_info, self.is_released)
    }

    pub fn show_buttons(
        ui: &mut egui::Ui,
        f: &mut File,
        depth: usize,
        selected: Option<PathBuf>,
    ) -> (Option<AudioInfo>, bool, Option<AudioInfo>) {
        let is_dir = f.entry.is_dir();
        let mut audio_info = None;
        let mut is_released = false;
        let mut selected_info = None;

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
        let is_selected = selected.clone().is_some_and(|f| f == clone);
        let label_response = left_aligned_selectable(
            ui,
            format!("{}{} {}", " ".repeat(depth * 2), icon, name),
            is_selected,
        );

        // On click
        if label_response.clicked() {
            if is_dir {
                f.open = !f.open;
            } else if is_audio {
                selected_info = AUDIO_ANALYSIS_CACHE.get_or_analyze(f.entry.clone());
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
                let (child_audio_info, released, child_selected) =
                    Self::show_buttons(ui, child, depth + 1, selected.clone());
                if child_audio_info.is_some() {
                    audio_info = child_audio_info;
                }
                if child_selected.is_some() {
                    selected_info = child_selected;
                }
                is_released = is_released || released;
            }
        }

        (audio_info, is_released, selected_info)
    }

    fn preview(&mut self, ui: &mut Ui, tx: &mut Producer<GuiToPlayerMsg>) {
        let (response, painter) =
            ui.allocate_painter(Vec2::new(ui.available_width(), 50.), Sense::click());
        let rect = response.rect;
        if let Some(info) = &self.selected
            && let Ok(data) = info.data.lock()
        {
            if response.clicked()
                && let Some(mouse_pos) = response.interact_pointer_pos()
            {
                let _ = tx.push(GuiToPlayerMsg::SeekPreview(
                    ((mouse_pos.x - rect.left()) / rect.width() * info.num_samples.unwrap() as f32)
                        .round() as usize,
                ));
            }

            painter.rect_filled(response.rect, 0., Color32::WHITE);
            let mut shapes = vec![];
            self.waveform.paint(
                &mut shapes,
                response.rect,
                data,
                0.,
                1.,
                info.num_samples.unwrap(),
            );
            let x = response.rect.left()
                + self.preview_position as f32 / info.num_samples.unwrap() as f32
                    * response.rect.width();
            shapes.push(Shape::line_segment(
                [
                    Pos2::new(x, response.rect.top()),
                    Pos2::new(x, response.rect.bottom()),
                ],
                Stroke::new(1.0, Color32::BLACK),
            ));
            painter.add(shapes);
        }
    }
}
