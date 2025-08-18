use std::f32::INFINITY;

use crate::{
    ProcessToGuiMsg,
    analysis::AudioInfo,
    cache::AUDIO_ANALYSIS_CACHE,
    components::{
        bottom_panel::{BOTTOM_BAR_HEIGHT, UIBottomPanel},
        clip::UIClip,
        grid::{MAX_RIGHT, MIN_LEFT, VIEW_WIDTH, WorkspaceGrid},
        left_panel::{DragPayload, UILeftPanel},
        top_bar::UITopBar,
        track::{DEFAULT_TRACK_HEIGHT, HANDLE_HEIGHT, TrackSoloState, UITrack},
        track_manager::TrackManager,
    },
    message::GuiToPlayerMsg,
    metrics::GlobalMetrics,
};
use eframe::egui::{self, Sense, Stroke};
use egui::{
    Align2, Color32, Context, FontId, Frame, Key, Layout, Margin, Painter, Pos2, Rangef, Rect,
    Response, ScrollArea, StrokeKind, Ui, Vec2,
};
use rtrb::{Consumer, Producer};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaybackState {
    Paused,
    Playing,
}

struct Multiselect {
    start_pos: f32,
    start_track_index: usize,
}

#[derive(Clone)]
struct DragState {
    elements: Vec<ClipDragState>,
    duplicate: bool,
    min_track_delta: i32,
}
#[derive(Clone)]
struct ClipDragState {
    clip: UIClip,
    mouse_delta: Pos2,
    track_index_delta: i32,
}

#[derive(Debug, Clone)]
struct Selection {
    pub clip_ids: Vec<String>,
    pub bounds: Option<SelectionBounds>,
}
#[derive(Debug, Clone, Copy)]
struct SelectionBounds {
    pub start_track_index: usize,
    pub start_pos: f32,
    pub end_track_index: usize,
    pub end_pos: f32,
}

impl Selection {
    pub fn reset(&mut self) {
        self.bounds = None;
        self.clip_ids.clear();
    }

    pub fn new() -> Self {
        Self {
            bounds: None,
            clip_ids: vec![],
        }
    }
}

const TOP_BAR_HEIGHT: f32 = 30.;
const LIMIT_WIDTH: f32 = 3.;
const DEFAULT_CLIP_COLOR: Color32 = Color32::GRAY;

pub struct Workspace {
    // Communicate
    to_player_tx: Producer<GuiToPlayerMsg>,
    from_player_rx: Consumer<ProcessToGuiMsg>,
    // Global state
    pub bpm: f32,
    pub playback_position: f32,
    // Navigation state
    grid: WorkspaceGrid,
    // Layout
    multiselect_start: Option<Multiselect>,
    clicked_pos: Option<Pos2>,
    // Manage tracks in the workspace
    track_manager: TrackManager,
    master_track: UITrack,
    sample_preview: Option<UIClip>,

    drag_state: Option<DragState>,

    selected_clips: Selection,

    playback_state: PlaybackState,

    metrics: GlobalMetrics,

    y_offset: f32,

    bottom_panel: UIBottomPanel,
    left_panel: UILeftPanel,
    top_bar: UITopBar,
}

impl Workspace {
    pub fn show(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("top-bar")
            .resizable(false)
            .show(ctx, |ui| {
                self.top_bar
                    .ui(ui, self.playback_state, self.bpm, &mut |new| {
                        let _ = self.to_player_tx.push(GuiToPlayerMsg::UpdateBPM(new));
                        self.bpm = new;
                    });
            });

        egui::TopBottomPanel::bottom("bottom-panel")
            .height_range(Rangef::new(10. + BOTTOM_BAR_HEIGHT, 400.))
            .resizable(true)
            .frame(Frame::new().inner_margin(Margin::ZERO))
            .show_animated(ctx, self.bottom_panel.open, |ui| {
                ui.set_height(ui.available_height());
                if let Some(id) = self.track_manager.selected_track.clone()
                    && let Some(track) = self.track_manager.tracks.iter_mut().find(|t| t.id == id)
                    && let Some(metrics) = &mut self.metrics.tracks.get_mut(&id)
                {
                    self.bottom_panel
                        .ui(ui, track, metrics, &mut self.to_player_tx);
                }
            });

        egui::SidePanel::left("left-pannel")
            .min_width(10.)
            .max_width(400.)
            .frame(
                Frame::new()
                    .inner_margin(Margin {
                        bottom: 0,
                        left: 0,
                        right: 1,
                        top: 0,
                    })
                    .fill(ctx.style().visuals.panel_fill)
                    .corner_radius(4.0),
            )
            .default_width(220.)
            .show_animated(ctx, true, |ui| {
                self.left_panel.ui(ui, &mut self.to_player_tx);
            });

        egui::CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).inner_margin(Margin::ZERO))
            .show(ctx, |ui| {
                self.ui(ui);
            });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.handle_messages();
        self.watch_inputs(ui);

        if self.playback_state == PlaybackState::Playing {
            ui.ctx().request_repaint();
        }

        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), ui.available_height()),
            Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.vertical(|ui| {
                    ui.painter().text(
                        Pos2::new(ui.max_rect().right() - 3., ui.max_rect().top() + 16.),
                        Align2::RIGHT_BOTTOM,
                        format!("{:.0}%", self.metrics.latency * 100.),
                        FontId::new(10., egui::FontFamily::Proportional),
                        ui.visuals().strong_text_color(),
                    );
                    ui.painter().text(
                        Pos2::new(ui.max_rect().right() - 3., ui.max_rect().top() + 28.),
                        Align2::RIGHT_BOTTOM,
                        format!(
                            "FPS: {:.1}",
                            1.0 / ui.ctx().input(|i| i.stable_dt).max(1e-5)
                        ),
                        FontId::new(10., egui::FontFamily::Proportional),
                        ui.visuals().strong_text_color(),
                    );
                    self.navigation_bar(ui);
                    let scroll = ScrollArea::vertical()
                        .id_salt("workspace-scrollbar")
                        .max_height(ui.available_height())
                        .show(ui, |ui| {
                            let mut height = 100.;
                            for track in self.track_manager.tracks.iter() {
                                height += track.height + HANDLE_HEIGHT;
                            }

                            ui.allocate_ui_with_layout(
                                Vec2::new(ui.available_width(), height.max(ui.available_height())),
                                Layout::left_to_right(egui::Align::Min),
                                |ui| {
                                    // Grid area
                                    let (response, painter) = ui.allocate_painter(
                                        egui::Vec2::new(
                                            ui.available_width()
                                                - self.track_manager.track_width
                                                - LIMIT_WIDTH,
                                            ui.available_height(),
                                        ),
                                        Sense::click_and_drag(),
                                    );

                                    let viewport_rect = response.rect;

                                    if response.clicked() {
                                        self.selected_clips.reset();
                                        self.track_manager.selected_track = None;
                                        let _ =
                                            self.to_player_tx.push(GuiToPlayerMsg::PausePreview());
                                        self.left_panel.file_browser.preview_state =
                                            PlaybackState::Paused;
                                    }
                                    let scroll_delta = ui.input(|i| i.smooth_scroll_delta.x);

                                    if scroll_delta != 0.
                                        && let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos())
                                        && viewport_rect.contains(mouse_pos)
                                    {
                                        self.grid.scroll(-scroll_delta);
                                    }

                                    if response.double_clicked()
                                        && let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos())
                                    {
                                        self.playback_position =
                                            self.grid.x_to_beats(mouse_pos.x, viewport_rect);

                                        let _ = self
                                            .to_player_tx
                                            .push(GuiToPlayerMsg::SeekTo(self.playback_position));
                                    }
                                    let mut dragged_audio = None;
                                    let mut is_released = false;
                                    if let Some(payload) =
                                        response.dnd_hover_payload::<DragPayload>()
                                        && let DragPayload::File(audio) = payload.as_ref()
                                    {
                                        dragged_audio = Some(audio.clone());
                                    }

                                    if let Some(payload) =
                                        response.dnd_release_payload::<DragPayload>()
                                        && let DragPayload::File(audio) = payload.as_ref()
                                    {
                                        dragged_audio = Some(audio.clone());
                                        is_released = true;
                                    }

                                    for dropped in ui.input(|i| i.raw.dropped_files.clone()) {
                                        if let Some(path) = dropped.path
                                            && path.extension().is_some_and(|ext| {
                                                ["mp3", "wav"].contains(&&ext.to_str().unwrap())
                                            })
                                        {
                                            dragged_audio =
                                                AUDIO_ANALYSIS_CACHE.get_or_analyze(path);
                                            is_released = true;
                                        }
                                    }

                                    // Do not draw after clips
                                    self.resize_handle(ui);
                                    self.track_manager.track_panel(
                                        ui,
                                        &mut self.metrics,
                                        &mut self.bottom_panel,
                                        &mut self.to_player_tx,
                                    );
                                    // Draw grid & clips
                                    self.grid.paint(&painter, viewport_rect);
                                    self.paint_clips(ui, viewport_rect);
                                    self.track_manager.paint_tracks(&painter, viewport_rect);
                                    self.paint_preview_sample(
                                        ui,
                                        viewport_rect,
                                        dragged_audio,
                                        is_released,
                                    );
                                    self.paint_playback_cursor(&painter, viewport_rect);
                                    self.handle_multiselect(ui, response);
                                    self.scrollbar(viewport_rect, &painter, ui);
                                },
                            )
                        });
                    self.y_offset = scroll.state.offset.y;

                    ui.scope_builder(
                        egui::UiBuilder::new().max_rect(Rect::from_min_size(
                            Pos2::new(
                                scroll.inner_rect.left(),
                                scroll.inner_rect.bottom() - self.master_track.height,
                            ),
                            Vec2::new(scroll.inner_rect.width(), self.master_track.height),
                        )),
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.add_space(ui.available_width() - self.track_manager.track_width);
                                self.master_track.ui(
                                    ui,
                                    self.metrics.master.clone(),
                                    TrackSoloState::NotSoloing,
                                    false,
                                );
                            })
                        },
                    );
                });
            },
        );
    }

    fn scrollbar(&mut self, rect: Rect, painter: &Painter, ui: &mut Ui) {
        let x = rect.left() + self.grid.left * rect.width() / (MAX_RIGHT - MIN_LEFT);
        let height = 6.;
        let scroll_rect = Rect::from_min_size(
            Pos2::new(x, rect.bottom() - height - 2.),
            Vec2::new(80., height),
        );
        let response = ui.interact(scroll_rect, "scrollbar".into(), Sense::click_and_drag());
        painter.rect_filled(scroll_rect, 2., Color32::from_gray(80));

        if response.dragged() {
            self.grid.scroll(response.drag_delta().x);
        }
    }

    fn paint_playback_cursor(&self, painter: &Painter, rect: Rect) {
        let x = self.grid.beats_to_x(self.playback_position, rect) - 0.5;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.0, Color32::from_white_alpha(200)),
        );
    }

    fn resize_handle(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(
            egui::Vec2::new(LIMIT_WIDTH, ui.available_height()),
            Sense::drag(),
        );

        painter.rect_filled(response.rect, 0., egui::Color32::from_gray(40));

        if response.hovered() {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
        }

        if response.dragged() {
            self.track_manager.track_width -= response.drag_delta().x;
            self.track_manager.track_width = self.track_manager.track_width.clamp(30., 300.);
        }
    }

    fn paint_clips(&mut self, ui: &mut Ui, viewport: egui::Rect) {
        let mut y = viewport.top();
        let mut dragged_track_index = None;
        let mut dragged_clip_index = None;

        for (track_index, track) in self.track_manager.tracks.iter_mut().enumerate() {
            let height = track.height;
            if y + track.height < viewport.top() + self.y_offset
                || y > self.y_offset + viewport.height() + viewport.top()
            {
                y += track.height + HANDLE_HEIGHT;
                continue;
            }

            for (clip_index, clip) in track.clips.iter_mut().enumerate() {
                if let Some(duration) = clip.duration() {
                    let width = self.grid.duration_to_width(duration, self.bpm);
                    let x = self.grid.beats_to_x(clip.position, viewport);

                    // Check sample is in frame
                    if !(x + width < viewport.left() || x > viewport.right()) {
                        let response = clip.ui(
                            ui,
                            Pos2::new(x, y),
                            Vec2::new(width, height),
                            viewport,
                            &self.grid,
                            self.bpm,
                            track.id.clone(),
                            self.selected_clips.clip_ids.contains(&clip.id()),
                            &mut self.to_player_tx,
                            !track.closed,
                            track.color,
                        );

                        if response.clicked() {
                            if self.selected_clips.clip_ids.contains(&clip.id()) {
                                self.selected_clips.reset();
                            } else {
                                self.selected_clips.bounds = None;
                                self.selected_clips.clip_ids = vec![clip.id()];
                            }
                        }

                        if dragged_clip_index.is_none()
                            && self.drag_state.is_none()
                            && response.dragged()
                        {
                            dragged_clip_index = Some(clip_index);
                            dragged_track_index = Some(track_index);
                        }
                    }
                };
            }
            y += track.height + HANDLE_HEIGHT;
        }

        self.handle_dragged_clips(ui, dragged_track_index, dragged_clip_index, viewport);
    }

    fn handle_dragged_clips(
        &mut self,
        ui: &mut Ui,
        dragged_track_index: Option<usize>,
        dragged_clip_index: Option<usize>,
        viewport: Rect,
    ) {
        let mouse_pos = ui.ctx().input(|i| i.pointer.hover_pos());
        // Create dragging object
        if self.drag_state.is_none()
            && let Some(old_track) = dragged_track_index
            && let Some(s) = dragged_clip_index
            && let Some(mouse_pos) = mouse_pos
        {
            // id of dragged clip
            let dragged_id = self.track_manager.tracks[old_track].clips[s].id();
            self.selected_clips.bounds = None;
            if !self.selected_clips.clip_ids.contains(&dragged_id) {
                self.selected_clips.clip_ids =
                    vec![self.track_manager.tracks[old_track].clips[s].id()];
            }
            let mut elements = Vec::new();
            let mut new_selected_clips = Vec::new();
            let duplicate = ui.input(|i| i.modifiers.ctrl);
            let mut y = viewport.top();
            let mut min_track_delta = 0;
            for (track_index, track) in self.track_manager.tracks.clone().iter().enumerate() {
                for clip in track.clips.iter() {
                    if self.selected_clips.clip_ids.contains(&clip.id()) {
                        let new_clip = if duplicate {
                            clip.clone_with_new_id()
                        } else {
                            self.track_manager.tracks[track_index]
                                .clips
                                .retain(|c| c.id() != clip.id());
                            clip.clone()
                        };
                        // Update selected clips
                        new_selected_clips.push(new_clip.id());
                        let track_index_delta = track_index as i32 - old_track as i32;
                        min_track_delta = min_track_delta.min(track_index_delta);
                        let x = self.grid.beats_to_x(clip.position, viewport);
                        elements.push(ClipDragState {
                            clip: new_clip,
                            mouse_delta: Pos2::new(mouse_pos.x - x, mouse_pos.y - y),
                            track_index_delta,
                        });
                    }
                }
                y += track.height + HANDLE_HEIGHT;
            }
            self.selected_clips.clip_ids = new_selected_clips;
            self.drag_state = Some(DragState {
                elements,
                duplicate,
                min_track_delta,
            });
        }
        // Render clips while dragging
        if let Some(mut drag_state) = self.drag_state.take()
            && let Some(mouse_pos) = mouse_pos
        {
            // Find track at mouse position
            let (t, _) = self.track_manager.find_track_at(viewport, mouse_pos.y);
            let mouse_track_index =
                t.unwrap_or(self.track_manager.tracks.len())
                    .max(-drag_state.min_track_delta as usize) as i32;

            // Find nearest grid to snap to
            let mut beat_delta: f32 = INFINITY;
            for element in drag_state.elements.iter() {
                let new_position = self
                    .grid
                    .x_to_beats(mouse_pos.x - element.mouse_delta.x, viewport);
                let snapped_position = self.grid.snap_at_grid(new_position);
                if let Some(pos) = snapped_position
                    && (pos - new_position).abs() < beat_delta.abs()
                {
                    beat_delta = pos - new_position;
                }
            }
            // No clip are snapped
            if beat_delta == INFINITY {
                beat_delta = 0.;
            }
            let mut track_indexes = Vec::new();
            for element in drag_state.elements.iter_mut() {
                if let Some(duration) = element.clip.duration() {
                    // Calculate track index
                    let track_index =
                        (mouse_track_index + element.track_index_delta).max(0) as usize;
                    track_indexes.push(track_index);
                    // Calculate y pos
                    let y = self.track_manager.get_track_y(track_index, viewport);
                    // Calculate width
                    let width = self.grid.duration_to_width(duration, self.bpm);
                    // Calculate x pos
                    let new_position = self
                        .grid
                        .x_to_beats(mouse_pos.x - element.mouse_delta.x, viewport)
                        + beat_delta;
                    let x = self.grid.beats_to_x(new_position, viewport);
                    element.clip.position = new_position;
                    let show_waveform = if track_index < self.track_manager.tracks.len() {
                        !self.track_manager.tracks[track_index].closed
                    } else {
                        true
                    };
                    let color = if track_index < self.track_manager.tracks.len() {
                        self.track_manager.tracks[track_index].color
                    } else {
                        DEFAULT_CLIP_COLOR
                    };
                    let height = if track_index < self.track_manager.tracks.len() {
                        self.track_manager.tracks[track_index].height
                    } else {
                        DEFAULT_TRACK_HEIGHT
                    };
                    let size = Vec2::new(width, height);
                    // Render clip
                    let _ = element.clip.ui(
                        ui,
                        Pos2::new(x, y),
                        size,
                        viewport,
                        &self.grid,
                        self.bpm,
                        "".to_string(),
                        true,
                        &mut self.to_player_tx,
                        show_waveform,
                        color,
                    );
                }
            }

            // Mouse released
            if !ui.input(|i| i.pointer.primary_down()) {
                for (i, element) in drag_state.elements.iter().enumerate() {
                    let track_index = track_indexes[i];
                    // Create
                    if track_index >= self.track_manager.tracks.len() {
                        for _ in 0..(track_index - self.track_manager.tracks.len() + 1) {
                            self.track_manager
                                .create_track("Audio Track", &mut self.to_player_tx);
                        }
                    }

                    let clone = element.clip.clone();

                    if drag_state.duplicate {
                        let _ = self.to_player_tx.push(GuiToPlayerMsg::AddClip(
                            self.track_manager.tracks[track_index].id.clone(),
                            clone.audio.path.clone(),
                            clone.position,
                            clone.id(),
                            clone.trim_start,
                            clone.trim_end,
                        ));
                    } else {
                        let _ = self.to_player_tx.push(GuiToPlayerMsg::MoveClip(
                            clone.id(),
                            self.track_manager.tracks[track_index].id.clone(),
                            clone.position,
                        ));
                    }

                    self.track_manager.tracks[track_index].add_clip(
                        clone,
                        self.bpm,
                        &mut self.to_player_tx,
                    );
                }

                self.drag_state = None;
            } else if !drag_state.duplicate || ui.input(|i| i.modifiers.ctrl) {
                self.drag_state = Some(drag_state);
            }
        }
    }

    fn watch_inputs(&mut self, ui: &mut Ui) {
        let mut new_selected = vec![];
        let mut updated = false;

        if ui.input(|i| i.key_pressed(egui::Key::Space)) {
            if self.playback_state == PlaybackState::Playing {
                let _ = self.to_player_tx.push(GuiToPlayerMsg::Pause);
                self.playback_state = PlaybackState::Paused
            } else {
                let _ = self.to_player_tx.push(GuiToPlayerMsg::Play);
                self.playback_state = PlaybackState::Playing;
            }
        }

        if ui.input(|i| i.key_pressed(Key::D) && i.modifiers.ctrl) {
            // Duplicate clips
            for track in self.track_manager.tracks.iter_mut() {
                let mut new_clips = Vec::new();
                for id in self.selected_clips.clip_ids.iter() {
                    let clip = track.clips.iter().find(|c| c.id() == *id);
                    if let Some(clip) = clip {
                        let mut duplicated_clip = clip.clone_with_new_id();

                        if let Some(bounds) = self.selected_clips.bounds {
                            duplicated_clip.trim_start_at(
                                bounds.start_pos.max(duplicated_clip.position),
                                self.bpm,
                            );
                            duplicated_clip.trim_end_at(
                                bounds.end_pos.min(duplicated_clip.end(self.bpm)),
                                self.bpm,
                            );
                            duplicated_clip.position += bounds.end_pos - bounds.start_pos;
                        } else {
                            duplicated_clip.position = clip.end(self.bpm);
                        }
                        new_selected.push(duplicated_clip.id().clone());
                        new_clips.push(duplicated_clip);
                        updated = true;
                    }
                }
                // Update track with new clips
                if new_clips.len() > 0 {
                    track.add_clips(new_clips, self.bpm, &mut self.to_player_tx);
                }
            }

            if let Some(bounds) = &mut self.selected_clips.bounds {
                let bound_size = bounds.end_pos - bounds.start_pos;
                bounds.start_pos = bounds.end_pos;
                bounds.end_pos += bound_size;
            }
        } else if ui
            .input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
        {
            // Delete
            for track in self.track_manager.tracks.iter_mut() {
                track.delete_ids(self.selected_clips.clip_ids.clone());

                let _ = self.to_player_tx.push(GuiToPlayerMsg::RemoveClip(
                    self.selected_clips.clip_ids.clone(),
                ));
            }
        } else if ui.input(|i| i.key_pressed(Key::K) && i.modifiers.ctrl) {
            // Cut clips
            let mut added_clip = vec![];

            for (index, track) in self.track_manager.tracks.iter_mut().enumerate() {
                for clip in track.clips.iter_mut() {
                    if clip.position < self.playback_position
                        && self.playback_position < clip.end(self.bpm)
                        && (self.selected_clips.clip_ids.is_empty()
                            || self.selected_clips.clip_ids.contains(&clip.id()))
                    {
                        added_clip.push((clip.clone_with_new_id(), index));
                        clip.trim_end_at(self.playback_position, self.bpm);
                        let _ = self.to_player_tx.push(GuiToPlayerMsg::ResizeClip(
                            clip.id(),
                            clip.trim_start,
                            clip.trim_end,
                        ));
                    }
                }
            }

            for (mut clip, track) in added_clip {
                clip.trim_start_at(self.playback_position, self.bpm);
                let _ = self.to_player_tx.push(GuiToPlayerMsg::AddClip(
                    self.track_manager.tracks[track].id.clone(),
                    clip.audio.path.clone(),
                    clip.position,
                    clip.id(),
                    clip.trim_start,
                    clip.trim_end,
                ));
                self.track_manager.tracks[track].clips.push(clip);
            }
        } else if ui.input(|i| i.key_pressed(Key::J) && i.modifiers.ctrl) {
            self.bottom_panel.open = !self.bottom_panel.open;
        }

        if updated {
            self.selected_clips.clip_ids = new_selected;
        }
    }

    fn paint_preview_sample(
        &mut self,
        ui: &mut Ui,
        rect: egui::Rect,
        dragged_audio_info: Option<AudioInfo>,
        is_released: bool,
    ) {
        // Paint Preview sample
        if let Some(audio_info) = dragged_audio_info
            && let Some(duration) = audio_info.duration
            && let Some(mouse_pos) = ui.ctx().input(|i| i.pointer.hover_pos())
            && rect.contains(mouse_pos)
        {
            // Calculate grid position in beats
            let position = self.grid.x_to_beats(mouse_pos.x, rect);
            let snapped_position = self.grid.snap_at_grid_with_default(position);
            // Create preview if not done yet
            if self.sample_preview.is_none() {
                self.sample_preview = Some(UIClip::new(audio_info, snapped_position));
            }

            let mouse_y = mouse_pos.y;

            // Find corresponding track
            let (track_index, track_y) = self.track_manager.find_track_at(rect, mouse_y);
            let height = if let Some(t) = track_index {
                self.track_manager.tracks[t].height
            } else {
                DEFAULT_TRACK_HEIGHT
            };
            let show_waveform = if let Some(t) = track_index {
                !self.track_manager.tracks[t].closed
            } else {
                true
            };
            let width = self.grid.duration_to_width(duration, self.bpm);

            if let Some(sample_preview) = self.sample_preview.as_mut() {
                sample_preview.position = snapped_position;
                sample_preview.ui(
                    ui,
                    Pos2::new(self.grid.beats_to_x(snapped_position, rect), track_y),
                    Vec2::new(width, height),
                    rect,
                    &self.grid,
                    self.bpm,
                    "".to_string(),
                    false,
                    &mut self.to_player_tx,
                    show_waveform,
                    DEFAULT_CLIP_COLOR,
                );
            }
        };

        if is_released
            && let Some(mouse_pos) = ui.ctx().input(|i| i.pointer.hover_pos())
            && let Some(preview) = self.sample_preview.clone()
        {
            // remove preview
            self.sample_preview = None;
            // add clip
            if rect.contains(mouse_pos) {
                let (track_index, _) = self.track_manager.find_track_at(rect, mouse_pos.y);
                let index;
                if let Some(track_index) = track_index {
                    index = track_index;
                } else {
                    // Create new track
                    self.track_manager
                        .create_track("Audio Track", &mut self.to_player_tx);
                    index = self.track_manager.tracks.len() - 1;
                }
                let id = preview.id();
                let _ = self.to_player_tx.push(GuiToPlayerMsg::AddClip(
                    self.track_manager.tracks[index].id.clone(),
                    preview.audio.path.clone(),
                    preview.position,
                    id,
                    preview.trim_start,
                    preview.trim_end,
                ));
                self.track_manager.tracks[index].add_clip(
                    preview,
                    self.bpm,
                    &mut self.to_player_tx,
                );
            }
        }
    }

    fn navigation_bar(&mut self, ui: &mut Ui) {
        // Rectangle for zoom control
        let zoom_rect = egui::Rect::from_min_size(
            egui::pos2(ui.min_rect().left(), ui.min_rect().top()),
            egui::vec2(
                ui.available_width() - self.track_manager.track_width - LIMIT_WIDTH,
                TOP_BAR_HEIGHT,
            ),
        );
        let (zoom_response, painter) = ui.allocate_painter(
            Vec2::new(
                ui.available_width() - self.track_manager.track_width - LIMIT_WIDTH,
                TOP_BAR_HEIGHT,
            ),
            Sense::click_and_drag(),
        );

        // Draw the zoom control rectangle
        painter.rect_filled(zoom_rect, 0.0, egui::Color32::from_gray(80));

        self.grid.draw_labels(&painter, zoom_rect);

        // Handle zoom
        if zoom_response.hovered() {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeVertical);
            let mouse_x = zoom_response
                .hover_pos()
                .map(|pos| pos.x - zoom_rect.left())
                .unwrap_or(VIEW_WIDTH / 2.0);
            self.grid
                .zoom_and_drag_at(mouse_x, ui.input(|i| i.smooth_scroll_delta));
        }
    }

    fn handle_multiselect(&mut self, ui: &mut Ui, response: Response) {
        if ui.input(|i| i.pointer.primary_down()) {
            self.clicked_pos = response.interact_pointer_pos();
        }

        if response.drag_started()
            && let Some(mouse_pos) = self.clicked_pos
        {
            let (track_index, _) = self.track_manager.find_track_at(response.rect, mouse_pos.y);
            let beat_pos = self.grid.x_to_beats(mouse_pos.x, response.rect);
            let snapped = self.grid.snap_at_grid_with_threshold_default(beat_pos, 1.);
            if self.track_manager.tracks.len() > 0 {
                let index = track_index.unwrap_or(self.track_manager.tracks.len() - 1);
                self.multiselect_start = Some(Multiselect {
                    start_pos: snapped,
                    start_track_index: index,
                });
            }
        }
        if !response.dragged() {
            self.multiselect_start = None;
        }

        if let Some(start) = &self.multiselect_start
            && let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos())
        {
            let (current_track_index, _) =
                self.track_manager.find_track_at(response.rect, mouse_pos.y);
            let current_pos = self.grid.snap_at_grid_with_threshold_default(
                self.grid.x_to_beats(mouse_pos.x, response.rect),
                1.0,
            );

            let min_index = current_track_index
                .unwrap_or(self.track_manager.tracks.len() - 1)
                .min(start.start_track_index);
            let max_index = current_track_index
                .unwrap_or(self.track_manager.tracks.len() - 1)
                .max(start.start_track_index);

            let min_pos = current_pos.min(start.start_pos);
            let max_pos = current_pos.max(start.start_pos);

            let mut min_point = Pos2::ZERO;
            let mut max_point = Pos2::ZERO;
            self.selected_clips.reset();
            self.selected_clips.bounds = Some(SelectionBounds {
                start_track_index: min_index,
                start_pos: min_pos,
                end_track_index: max_index,
                end_pos: max_pos,
            });
            let mut y = response.rect.top();
            for (track_index, track) in self.track_manager.tracks.iter().enumerate() {
                if track_index == min_index {
                    min_point = Pos2::new(self.grid.beats_to_x(min_pos, response.rect), y);
                }
                if min_index <= track_index && track_index <= max_index {
                    for clip in track.clips.iter() {
                        let end = clip.end(self.bpm);
                        if end >= min_pos && clip.position < max_pos {
                            self.selected_clips.clip_ids.push(clip.id());
                        }
                    }
                }
                y += track.height;
                if track_index == max_index {
                    max_point = Pos2::new(self.grid.beats_to_x(max_pos, response.rect), y);
                }
                y += HANDLE_HEIGHT;
            }
            let select_rect = Rect::from_min_max(min_point, max_point);

            ui.painter().rect_stroke(
                select_rect,
                0.,
                Stroke::new(1. / ui.pixels_per_point(), Color32::from_white_alpha(250)),
                StrokeKind::Inside,
            );
        }

        if let Some(bounds) = self.selected_clips.bounds {
            let min_point = Pos2::new(
                self.grid.beats_to_x(bounds.start_pos, response.rect),
                self.track_manager
                    .get_track_y(bounds.start_track_index, response.rect),
            );

            let max_point = Pos2::new(
                self.grid.beats_to_x(bounds.end_pos, response.rect),
                self.track_manager
                    .get_track_y(bounds.end_track_index, response.rect)
                    + self.track_manager.tracks[bounds.end_track_index].height,
            );
            let rect = Rect::from_min_max(min_point, max_point);
            let painter = ui.painter_at(rect);
            painter.rect(
                rect,
                2.0,
                Color32::LIGHT_BLUE.gamma_multiply(0.1),
                Stroke::new(1. / ui.pixels_per_point(), Color32::WHITE),
                StrokeKind::Inside,
            );
        }
    }

    fn handle_messages(&mut self) {
        while let Ok(msg) = self.from_player_rx.pop() {
            match msg {
                ProcessToGuiMsg::PlaybackPos(pos) => {
                    self.playback_position = pos;
                    self.playback_state = PlaybackState::Playing;
                }
                ProcessToGuiMsg::Metrics(metrics) => self.metrics = metrics,
                ProcessToGuiMsg::PreviewPos(pos) => {
                    self.left_panel.file_browser.preview_position = pos;
                }
            }
        }
    }

    pub fn new(
        to_player_tx: Producer<GuiToPlayerMsg>,
        from_player_rx: Consumer<ProcessToGuiMsg>,
    ) -> Self {
        Self {
            from_player_rx,
            clicked_pos: None,
            to_player_tx,
            bpm: 120.0,
            sample_preview: None,
            grid: WorkspaceGrid::new(),
            drag_state: None,
            selected_clips: Selection::new(),
            playback_position: 0.,
            playback_state: PlaybackState::Paused,
            metrics: GlobalMetrics::new(),
            multiselect_start: None,
            track_manager: TrackManager::new(),
            master_track: UITrack::new("master", "Master"),

            y_offset: 0.,
            left_panel: UILeftPanel::new(),
            bottom_panel: UIBottomPanel::new(),
            top_bar: UITopBar::new(),
        }
    }
}
