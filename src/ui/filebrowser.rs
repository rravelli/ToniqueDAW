use crate::{
    analysis::AudioInfo,
    cache::AUDIO_ANALYSIS_CACHE,
    config::{load_work_dir, save_work_dir},
    core::state::ToniqueProjectState,
    ui::{
        buttons::left_aligned_selectable, panels::left_panel::DragPayload, waveform::UIWaveform,
        workspace::PlaybackState,
    },
};
use egui::{
    Color32, CursorIcon, Frame, Key, Label, Pos2, Rect, Response, RichText, ScrollArea, Sense,
    Shape, Spinner, Stroke, Ui, Vec2, mutex::Mutex,
};
use rfd::FileDialog;
use std::{ffi::OsStr, fs::read_dir, ops::Range, path::PathBuf, sync::Arc, thread};

const PREVIEW_WINDOW_HEIGHT: f32 = 60.;

#[derive(Debug, Clone)]
pub struct FileNode {
    pub entry: PathBuf,
    pub open: bool,
    children: Option<Vec<FileNode>>,
}

pub fn filter_files(file: &mut FileNode, query: &str) -> Option<FileNode> {
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
        let matching_children: Vec<FileNode> = children
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
        Some(FileNode {
            entry: file.entry.clone(),
            open: file.open,
            children: filtered_children,
        })
    } else {
        None
    }
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

impl FileNode {
    fn get_children(&mut self) -> &mut Vec<FileNode> {
        if self.children.is_none() {
            self.children = Some(Self::read_entries(self.entry.clone()));
        }
        self.children.as_mut().unwrap()
    }

    fn read_entries(dir: PathBuf) -> Vec<FileNode> {
        let mut entries = read_dir(dir)
            .map(|read_dir| {
                read_dir
                    .filter_map(|entry| entry.ok())
                    .map(|f| f.path())
                    .filter(|path| path.is_dir() || is_audio_file(path))
                    .map(|entry| FileNode {
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

pub struct FileBrowser {
    root: Vec<FileNode>,
    total_rows: usize,
    selected_audio: Option<AudioInfo>,
    selected_index: Option<usize>,
    waveform: UIWaveform,
    search_query: String,
    pub search_result: Arc<Mutex<Option<FileNode>>>,
    pub search_in_progress: Arc<Mutex<bool>>,
}

fn show_files(
    curr: &mut FileNode,
    ui: &mut Ui,
    index: &mut usize,
    depth: usize,
    range: &Range<usize>,
    selected_audio: &mut Option<AudioInfo>,
    selected_index: &mut Option<usize>,
    clicked: &mut bool,
) {
    if range.contains(&*index) {
        let res = paint_item(ui, curr, depth, selected_audio, selected_index, *index);
        if res.clicked() {
            *clicked = true;
        }
    }
    if curr.entry.is_dir() && curr.open {
        for child in curr.get_children() {
            *index += 1;
            show_files(
                child,
                ui,
                index,
                depth + 1,
                range,
                selected_audio,
                selected_index,
                clicked,
            );
        }
    }
}

fn paint_item(
    ui: &mut Ui,
    file: &mut FileNode,
    depth: usize,
    selected_audio: &mut Option<AudioInfo>,
    selected_index: &mut Option<usize>,
    index: usize,
) -> Response {
    let is_dir = file.entry.is_dir();
    let selected = selected_index.is_some_and(|idx| idx == index);
    let extension = if let Some(ext) = file.entry.extension() {
        ext.to_str().unwrap()
    } else {
        ""
    };

    let is_audio = ["mp3", "wav"].contains(&extension);

    let icon = if is_dir {
        if file.open {
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
        .entry
        .file_name()
        .unwrap_or(OsStr::new(""))
        .to_string_lossy();
    let button_response = left_aligned_selectable(
        ui,
        format!("{}{} {}", " ".repeat(depth * 2), icon, name),
        selected,
    );

    if button_response.clicked() || (selected && ui.input(|r| r.key_pressed(Key::Enter))) {
        file.open = !file.open;
        *selected_index = Some(index);
        *selected_audio = AUDIO_ANALYSIS_CACHE.get_or_analyze(file.entry.clone());
    }

    if selected {
        *selected_audio = AUDIO_ANALYSIS_CACHE.get_or_analyze(file.entry.clone());
        ui.scroll_to_rect(
            Rect::from_min_max(
                Pos2::new(button_response.rect.min.x, button_response.rect.min.y - 20.),
                Pos2::new(button_response.rect.max.x, button_response.rect.max.y + 20.),
            ),
            None,
        );
    }

    if button_response.dragged() {
        if is_audio
            && let Some(audio_info) = AUDIO_ANALYSIS_CACHE.get_or_analyze(file.entry.clone())
        {
            button_response.dnd_set_drag_payload(DragPayload::File(audio_info));
        }
    }

    button_response
}

impl FileBrowser {
    pub fn new() -> Self {
        Self {
            root: load_work_dir().map_or(vec![], |f| {
                vec![FileNode {
                    children: None,
                    entry: PathBuf::from(f),
                    open: false,
                }]
            }),
            search_query: "".into(),

            waveform: UIWaveform::new(),
            selected_audio: None,
            total_rows: 0,
            selected_index: None,
            search_in_progress: Arc::new(Mutex::new(false)),
            search_result: Arc::new(Mutex::new(None)),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        let mut new_total_rows = 0;
        let previous_selected = self.selected_audio.clone();
        self.update_index_position(ui);
        self.choose_dir_button(ui);
        ui.add_space(5.0);
        let in_progress = *self.search_in_progress.lock();
        if in_progress {
            ui.horizontal(|ui| {
                ui.add(Spinner::new().size(10.));
                ui.label("Searching");
            });
        }

        if ui.response().clicked_elsewhere() {
            self.selected_index = None;
            self.selected_audio = None;
        }
        let mut clicked = false;
        ui.vertical(|ui| {
            ui.set_height(ui.available_height() - PREVIEW_WINDOW_HEIGHT);
            ScrollArea::vertical().show_rows(ui, 16., self.total_rows, |ui, row_range| {
                if self.root.len() > 0 && !in_progress {
                    let result = self.search_result.clone();
                    let mut locked = result.lock();

                    let root = if let Some(r) = locked.as_mut() {
                        r
                    } else {
                        &mut self.root[0]
                    };
                    show_files(
                        root,
                        ui,
                        &mut new_total_rows,
                        0,
                        &row_range,
                        &mut self.selected_audio,
                        &mut self.selected_index,
                        &mut clicked,
                    );
                }
            });
        });
        self.preview_window(ui, state);
        // Gained focus
        if let Some(audio) = &self.selected_audio
            && (previous_selected.is_none_or(|prev| prev.path != audio.path) || clicked)
        {
            state.play_preview(audio.path.clone());
        }

        if state.preview_playback_state() == PlaybackState::Playing {
            ui.ctx().request_repaint();
            // Pause player when losing focus
            if self.selected_audio.is_none() {
                state.pause_preview();
            }
        }

        self.total_rows = new_total_rows + 1;
    }

    fn choose_dir_button(&mut self, ui: &mut Ui) {
        if ui.button("Select folder").clicked() {
            let picked_dir = FileDialog::new().pick_folder();
            if let Some(path) = picked_dir {
                save_work_dir(&path.to_str().unwrap());
                let mut file = FileNode {
                    children: None,
                    entry: path,
                    open: true,
                };
                file.get_children();
                self.root = vec![file];
            }
        };
    }

    fn update_index_position(&mut self, ui: &mut Ui) {
        if let Some(index) = self.selected_index.as_mut() {
            if ui.input(|i| i.key_pressed(Key::ArrowUp)) && *index > 0 {
                *index -= 1;
            } else if ui.input(|i| i.key_pressed(Key::ArrowDown)) && *index < self.total_rows - 1 {
                *index += 1;
            }
        }
    }

    fn preview_window(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        Frame::new()
            .stroke(Stroke::new(4.0, Color32::from_gray(100)))
            .show(ui, |ui| {
                ui.set_height(PREVIEW_WINDOW_HEIGHT - 2.0 * 4.0);
                let (response, painter) =
                    ui.allocate_painter(Vec2::new(ui.available_width(), 12.), Sense::click());
                let rect = response.rect;

                if let Some(info) = &self.selected_audio
                    && let Ok(data) = info.data.read()
                {
                    if response.clicked()
                        && let Some(mouse_pos) = response.interact_pointer_pos()
                    {
                        state.seek_preview(
                            ((mouse_pos.x - rect.left()) / rect.width()
                                * info.num_samples.unwrap() as f32)
                                .round() as usize,
                        );
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
                        + state.preview_position() as f32 / info.num_samples.unwrap() as f32
                            * response.rect.width();
                    shapes.push(Shape::line_segment(
                        [
                            Pos2::new(x, response.rect.top()),
                            Pos2::new(x, response.rect.bottom()),
                        ],
                        Stroke::new(1.0, Color32::BLACK),
                    ));

                    painter.add(shapes);
                    ui.add(
                        Label::new(RichText::new(info.name.clone()).strong().size(12.))
                            .selectable(false)
                            .wrap_mode(egui::TextWrapMode::Truncate),
                    );
                    ui.add(
                        Label::new(
                            RichText::new(format!(
                                "Length: {:.3}s",
                                info.duration.unwrap().as_secs_f32()
                            ))
                            .size(10.),
                        )
                        .selectable(false)
                        .wrap_mode(egui::TextWrapMode::Truncate),
                    );
                    ui.add(
                        Label::new(
                            RichText::new(format!(
                                "Format: {:.1}kHz {}-bit",
                                info.sample_rate as f32 / 1000.,
                                info.bit_depth.unwrap_or(16)
                            ))
                            .size(10.),
                        )
                        .selectable(false)
                        .wrap_mode(egui::TextWrapMode::Truncate),
                    );
                    response.on_hover_cursor(CursorIcon::Crosshair);
                }
            });
    }

    pub fn trigger_search(&mut self, query: String) {
        self.search_query = query.clone();
        if query.is_empty() {
            *self.search_result.lock() = None;
            return;
        }
        let in_progress = self.search_in_progress.clone();
        let mut node = self.root[0].clone();
        let query_clone = query.clone();
        let result_arc = self.search_result.clone();
        *in_progress.lock() = true;
        thread::spawn(move || {
            let result = filter_files(&mut node, &query_clone);
            *result_arc.lock() = result;
            *in_progress.lock() = false;
        });
    }
}
