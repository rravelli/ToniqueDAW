use crate::{
    ProcessToGuiMsg,
    analysis::AudioInfo,
    cache::AUDIO_ANALYSIS_CACHE,
    core::{
        clip::ClipCore,
        state::ToniqueProjectState,
        track::{TrackCore, TrackSoloState},
    },
    message::GuiToPlayerMsg,
    ui::{
        clip::UIClipv2,
        grid::{MAX_RIGHT, MIN_LEFT, VIEW_WIDTH, WorkspaceGrid},
        panels::{
            bottom_panel::{BOTTOM_BAR_HEIGHT, UIBottomPanel},
            left_panel::{DragPayload, UILeftPanel},
            top_bar::UITopBar,
        },
        track::{DEFAULT_TRACK_HEIGHT, HANDLE_HEIGHT, UITrack},
        track_manager::TrackManager,
    },
};
use eframe::egui::{self, Sense, Stroke};
use egui::{
    Align2, Color32, Context, DragAndDrop, FontId, Frame, Key, Layout, Margin, Painter, Pos2,
    Rangef, Rect, Response, ScrollArea, StrokeKind, Ui, Vec2,
};
use rtrb::{Consumer, Producer};
use std::f32::INFINITY;

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
    clip: ClipCore,
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
const DEFAULT_CLIP_COLOR: Color32 = Color32::WHITE;

pub struct Workspace {
    // Navigation state
    grid: WorkspaceGrid,
    // Layout
    multiselect_start: Option<Multiselect>,
    clicked_pos: Option<Pos2>,
    // Manage tracks in the workspace
    track_manager: TrackManager,
    sample_preview: Option<ClipCore>,

    drag_state: Option<DragState>,

    selected_clips: Selection,

    y_offset: f32,
    // Ui elements
    bottom_panel: UIBottomPanel,
    left_panel: UILeftPanel,
    top_bar: UITopBar,
    // Global state of the app
    state: ToniqueProjectState,
}

impl Workspace {
    pub fn show(&mut self, ctx: &Context) {
        // Update state
        self.state.update();
        if self.state.playback_state() == PlaybackState::Playing {
            ctx.request_repaint();
        }

        egui::TopBottomPanel::top("top-bar")
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(Color32::from_gray(40))
                    .inner_margin(Margin::same(4)),
            )
            .show(ctx, |ui| {
                self.top_bar.ui(ui, &mut self.state);
            });

        egui::TopBottomPanel::bottom("bottom-panel")
            .height_range(Rangef::new(50. + BOTTOM_BAR_HEIGHT, 400.))
            .resizable(true)
            .frame(Frame::new().inner_margin(Margin::ZERO))
            .show_animated(ctx, self.bottom_panel.open, |ui| {
                ui.set_height(ui.available_height());

                if let Some(selected) = self.state.selected_track() {
                    self.bottom_panel.ui(ui, selected, &mut self.state);
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
                self.left_panel.ui(ui, &mut self.state);
            });

        egui::CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).inner_margin(Margin::ZERO))
            .show(ctx, |ui| {
                self.ui(ui);
            });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.handle_hot_keys(ui);

        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), ui.available_height()),
            Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.vertical(|ui| {
                    // ui.painter().text(
                    //     Pos2::new(ui.max_rect().right() - 3., ui.max_rect().top() + 16.),
                    //     Align2::RIGHT_BOTTOM,
                    //     format!("{:.0}%", self.state.metrics.latency * 100.),
                    //     FontId::new(10., egui::FontFamily::Proportional),
                    //     ui.visuals().strong_text_color(),
                    // );
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
                            for track in self.state.tracks() {
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
                                        self.state.deselect();
                                        self.state.pause_preview();
                                    }
                                    let scroll_delta = ui.input(|i| i.smooth_scroll_delta.x);

                                    if scroll_delta != 0.
                                        && let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos())
                                        && viewport_rect.contains(mouse_pos)
                                    {
                                        self.grid.scroll(-scroll_delta);
                                    }

                                    if response.clicked()
                                        && let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos())
                                    {
                                        self.state.set_playback_position(
                                            self.grid.x_to_beats(mouse_pos.x, viewport_rect),
                                        );
                                    }
                                    let mut dragged_audio = None;
                                    let mut is_released = false;
                                    if let Some(payload) =
                                        response.dnd_hover_payload::<DragPayload>()
                                        && let DragPayload::File(audio) = payload.as_ref()
                                    {
                                        dragged_audio = Some(audio.clone());
                                        if let Some(payload) =
                                            response.dnd_release_payload::<DragPayload>()
                                            && let DragPayload::File(audio) = payload.as_ref()
                                        {
                                            dragged_audio = Some(audio.clone());
                                            is_released = true;
                                        }
                                    }

                                    for dropped in ui.input(|i| i.raw.dropped_files.clone()) {
                                        if let Some(path) = dropped.path
                                            && path.extension().is_some_and(|ext| {
                                                ["mp3", "wav", "ogg"]
                                                    .contains(&&ext.to_str().unwrap())
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
                                        &mut self.bottom_panel,
                                        &mut self.state,
                                    );
                                    // Draw grid & clips
                                    self.grid.paint(&painter, viewport_rect);
                                    self.paint_clips(ui, viewport_rect);
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
                    let master_track = self.state.master_track();
                    ui.scope_builder(
                        egui::UiBuilder::new().max_rect(Rect::from_min_size(
                            Pos2::new(
                                scroll.inner_rect.left(),
                                scroll.inner_rect.bottom() - master_track.height,
                            ),
                            Vec2::new(scroll.inner_rect.width(), master_track.height),
                        )),
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.add_space(ui.available_width() - self.track_manager.track_width);
                                UITrack::new().ui(ui, &self.state.master_track(), &mut self.state);
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
        let x = self.grid.beats_to_x(self.state.playback_position(), rect) - 1.;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(2.0, Color32::from_white_alpha(160)),
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
        let mut dragged_clip = None;
        let tracks: Vec<_> = self.state.tracks().collect();
        let dragged_ids: Vec<_> = self
            .drag_state
            .as_ref()
            .map(|drag| drag.elements.iter().map(|e| &e.clip.id).collect())
            .unwrap_or_default();
        // Paint tracks ui from top to bottom
        for track in tracks {
            if y + track.height < viewport.top() + self.y_offset
                || y > self.y_offset + viewport.height() + viewport.top()
            {
                y += track.height + HANDLE_HEIGHT;
                continue;
            }
            for clip in track.clips.iter() {
                if let Some(duration) = clip.duration() {
                    let width = self.grid.duration_to_width(duration, self.state.bpm());
                    let x = self.grid.beats_to_x(clip.position, viewport);
                    let dragged = dragged_ids.contains(&&clip.id);
                    // Check clip is in frame
                    if !(x + width < viewport.left() || x > viewport.right()) {
                        let pos = Pos2::new(x, y);
                        let size = Vec2::new(width, track.height);
                        let color = if track.disabled() {
                            Color32::from_gray(100)
                        } else if dragged {
                            Color32::from_white_alpha(10)
                        } else {
                            track.color
                        };
                        // Render ui
                        let response = UIClipv2::new().ui(
                            ui,
                            pos,
                            size,
                            viewport,
                            &self.grid,
                            !dragged && self.selected_clips.clip_ids.contains(&clip.id),
                            clip,
                            &mut self.state,
                            !track.closed,
                            color,
                        );
                        // Select clip
                        if response.clicked() {
                            let shift = ui.input(|r| r.modifiers.shift);
                            if self.selected_clips.clip_ids.contains(&clip.id) {
                                if shift {
                                    self.selected_clips.clip_ids.retain(|id| *id != clip.id);
                                } else {
                                    self.selected_clips.reset();
                                }
                            } else {
                                self.selected_clips.bounds = None;
                                if shift {
                                    self.selected_clips.clip_ids.push(clip.id.clone());
                                } else {
                                    self.selected_clips.clip_ids = vec![clip.id.clone()];
                                }
                                self.state.select_track(&track.id);
                            }
                        }
                        // Keep track of the first clip dragged
                        if dragged_clip.is_none() && response.dragged() {
                            dragged_clip = Some(clip.clone());
                            dragged_track_index = Some(track.index);
                        }
                    }
                };
            }
            let track_rect = Rect::from_min_max(
                Pos2::new(viewport.left(), y),
                Pos2::new(viewport.right(), y + track.height),
            );

            let painter = ui.painter();
            // Check for drag and drop
            if let Some(pointer) = ui.input(|r| r.pointer.hover_pos())
                && track_rect.contains(pointer)
                && let Some(payload) = DragAndDrop::payload::<DragPayload>(&ui.ctx())
                && let DragPayload::Effect(_) = *payload
            {
                painter.rect_filled(track_rect, 1., Color32::from_white_alpha(100));
                self.bottom_panel.open = true;
                self.state.select_track(&track.id);
            }
            if let Some(pointer) = ui.input(|r| r.pointer.hover_pos())
                && ui.input(|i| i.pointer.any_released())
                && track_rect.contains(pointer)
                && let Some(payload) = DragAndDrop::payload::<DragPayload>(ui.ctx())
                && let DragPayload::Effect(id) = *payload
            {
                self.state.add_effect(&track.id, id, 0);
                DragAndDrop::take_payload::<DragPayload>(ui.ctx());
            }

            y += track.height;
            painter.line(
                vec![
                    Pos2::new(viewport.left(), y + HANDLE_HEIGHT / 2.),
                    Pos2::new(viewport.right(), y + HANDLE_HEIGHT / 2.),
                ],
                Stroke::new(HANDLE_HEIGHT, Color32::from_gray(60)),
            );
            y += HANDLE_HEIGHT;
        }

        self.handle_dragged_clips(ui, dragged_track_index, viewport, dragged_clip);
    }

    fn handle_dragged_clips(
        &mut self,
        ui: &mut Ui,
        dragged_track_index: Option<usize>,
        viewport: Rect,
        dragged_clip: Option<ClipCore>,
    ) {
        let mouse_pos = ui.ctx().input(|i| i.pointer.hover_pos());
        // Create dragging objects
        if self.drag_state.is_none()
            && let Some(clip) = dragged_clip
            && let Some(mouse_pos) = mouse_pos
            && let Some(old_track) = dragged_track_index
        {
            self.selected_clips.bounds = None;

            if !self.selected_clips.clip_ids.contains(&clip.id) {
                self.selected_clips.clip_ids = vec![clip.id];
            }
            let mut elements = Vec::new();
            let mut new_selected_clips = Vec::new();
            let duplicate = ui.input(|i| i.modifiers.ctrl);
            let mut y = viewport.top();
            let mut min_track_delta = 0;
            let tracks: Vec<_> = self.state.tracks().collect();
            for track in tracks {
                for clip in track.clips.iter() {
                    if self.selected_clips.clip_ids.contains(&clip.id) {
                        // Create a clone
                        let new_clip = if duplicate {
                            clip.clone_with_new_id()
                        } else {
                            clip.clone()
                        };
                        // Update selected clips
                        new_selected_clips.push(new_clip.clone().id);
                        let track_index_delta = track.index as i32 - old_track as i32;
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
            let (track, _) =
                self.track_manager
                    .find_track_at(viewport, &mut self.state, mouse_pos.y);

            // Index of the track at mouse position
            let mouse_track_index = track
                .map_or(self.state.track_len(), |t| t.index)
                .max(-drag_state.min_track_delta as usize)
                as i32;

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
                    let y = self
                        .track_manager
                        .get_track_y(track_index, viewport, &self.state);
                    // Calculate width
                    let width = self.grid.duration_to_width(duration, self.state.bpm());
                    // Calculate x pos
                    let new_position = self
                        .grid
                        .x_to_beats(mouse_pos.x - element.mouse_delta.x, viewport)
                        + beat_delta;
                    element.clip.position = new_position;
                    let x = self.grid.beats_to_x(new_position, viewport);

                    let mut show_waveform = true;
                    let mut color = DEFAULT_CLIP_COLOR;
                    let mut height = DEFAULT_TRACK_HEIGHT;
                    if let Some(t) = self.state.track_from_index(track_index) {
                        show_waveform = !t.closed;
                        color = t.color;
                        height = t.height;
                    }
                    let pos = Pos2::new(x, y);
                    let size = Vec2::new(width, height);
                    // Render Clip
                    UIClipv2::new().ui(
                        ui,
                        pos,
                        size,
                        viewport,
                        &self.grid,
                        true,
                        &element.clip,
                        &mut self.state,
                        show_waveform,
                        color,
                    );
                }
            }

            // Update state on mouse released
            if !ui.input(|i| i.pointer.primary_down()) {
                // self.state.begin_batch();
                let ids = drag_state
                    .elements
                    .iter()
                    .map(|e| e.clip.id.clone())
                    .collect();

                let mut tracks: Vec<_> = self.state.tracks().collect();
                self.state.begin_batch();
                for (i, element) in drag_state.elements.iter().enumerate() {
                    let length = tracks.len();
                    let track_index = track_indexes[i];
                    // Create
                    if track_index >= length {
                        for _ in 0..(track_index - length + 1) {
                            let new_track = TrackCore::new();
                            tracks.push(new_track.get_reference(
                                0,
                                false,
                                TrackSoloState::NotSoloing,
                            ));
                            self.state.add_track(new_track);
                        }
                    }

                    let clone = element.clip.clone();

                    let track = &tracks[track_index];
                    let track_id = track.id.clone();
                    if drag_state.duplicate {
                        self.state.add_clips(&track_id, vec![clone]);
                    } else {
                        self.state
                            .move_clip(&clone.id, &track_id, clone.position, &ids);
                    }
                }
                // NEW VERSION
                if drag_state.duplicate {
                    // self.state.add_clips(track_id, clips);
                } else {
                }
                self.state.commmit_batch();
                self.drag_state = None;
            } else if !drag_state.duplicate || ui.input(|i| i.modifiers.ctrl) {
                self.drag_state = Some(drag_state);
            }
        }
    }

    fn handle_hot_keys(&mut self, ui: &mut Ui) {
        // If other element focused do not check
        if ui.memory(|m| m.focused().is_some()) {
            return;
        }

        if ui.input(|i| i.focused && i.key_pressed(egui::Key::Space)) {
            if self.state.playback_state() == PlaybackState::Playing {
                self.state.pause();
            } else {
                self.state.play();
            }
        }

        if ui.input(|i| i.key_pressed(Key::D) && i.modifiers.ctrl) {
            // Duplicate clips
            if self.selected_clips.clip_ids.len() > 0 {
                self.state.duplicate_clips(
                    &self.selected_clips.clip_ids,
                    self.selected_clips.bounds.map(|b| (b.start_pos, b.end_pos)),
                );
                self.selected_clips.clip_ids.clear();
                if let Some(bounds) = &mut self.selected_clips.bounds {
                    let bound_size = bounds.end_pos - bounds.start_pos;
                    bounds.start_pos = bounds.end_pos;
                    bounds.end_pos += bound_size;
                }
            } else if let Some(selected) = self.state.selected_track() {
                self.state.duplicate_track(&selected.id);
            }
        } else if ui.memory(|m| m.focused().is_none())
            && ui.input(|i| {
                i.focused && i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)
            })
        {
            // Delete
            if self.selected_clips.clip_ids.len() > 0 {
                self.state.delete_clips(&self.selected_clips.clip_ids);
                self.selected_clips.reset();
            } else if let Some(selected) = self.state.selected_track() {
                self.state.delete_track(&selected.id);
            }
        } else if ui.input(|i| i.key_pressed(Key::K) && i.modifiers.ctrl) {
            // Cut clips
            for id in self.state.selected_tracks().clone() {
                self.state.cut_clip_at(&id, self.state.playback_position());
            }
        } else if ui.input(|i| i.key_pressed(Key::J) && i.modifiers.ctrl) {
            // Close bottom panel
            self.bottom_panel.open = !self.bottom_panel.open;
        } else if ui.input(|i| i.modifiers.ctrl && i.key_pressed(Key::Z)) {
            // Undo
            self.state.undo();
        } else if ui.input(|i| i.modifiers.ctrl && i.key_pressed(Key::Y)) {
            // Redo
            self.state.redo();
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
                self.sample_preview = Some(ClipCore::new(audio_info, snapped_position));
            }

            let mouse_y = mouse_pos.y;

            // Find corresponding track
            let (track, track_y) = self.track_manager.find_track_at(rect, &self.state, mouse_y);

            let height = if let Some(ref t) = track {
                t.height
            } else {
                DEFAULT_TRACK_HEIGHT
            };
            let show_waveform = if let Some(ref t) = track {
                !t.closed
            } else {
                true
            };
            let width = self.grid.duration_to_width(duration, self.state.bpm());

            if let Some(sample_preview) = self.sample_preview.as_mut() {
                sample_preview.position = snapped_position;
                let pos = Pos2::new(self.grid.beats_to_x(snapped_position, rect), track_y);
                let size = Vec2::new(width, height);
                UIClipv2::new().ui(
                    ui,
                    pos,
                    size,
                    rect,
                    &self.grid,
                    false,
                    &sample_preview,
                    &mut self.state,
                    show_waveform,
                    track.map_or(DEFAULT_CLIP_COLOR, |t| t.color),
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
                let (track, _) = self
                    .track_manager
                    .find_track_at(rect, &self.state, mouse_pos.y);

                let id = if let Some(t) = track {
                    t.id
                } else {
                    let new_track = TrackCore::new();
                    self.state.add_track(new_track.clone());
                    new_track.id
                };
                self.state.add_clips(&id, vec![preview]);
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
            let (track, _) =
                self.track_manager
                    .find_track_at(response.rect, &self.state, mouse_pos.y);

            let beat_pos = self.grid.x_to_beats(mouse_pos.x, response.rect);
            let snapped = self.grid.snap_at_grid_with_threshold_default(beat_pos, 1.);
            if self.state.track_len() > 0 {
                let index = track.map_or(self.state.track_len() - 1, |t| t.index);
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
            let (current_track, _) =
                self.track_manager
                    .find_track_at(response.rect, &self.state, mouse_pos.y);

            let current_pos = self.grid.snap_at_grid_with_threshold_default(
                self.grid.x_to_beats(mouse_pos.x, response.rect),
                1.0,
            );
            let length = self.state.track_len();
            let track_index = current_track.map_or(length - 1, |t| t.index);
            let min_index = track_index.min(start.start_track_index);
            let max_index = track_index.max(start.start_track_index);

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
            for (track_index, track) in self.state.tracks().enumerate() {
                if track_index == min_index {
                    min_point = Pos2::new(self.grid.beats_to_x(min_pos, response.rect), y);
                }
                if min_index <= track_index && track_index <= max_index {
                    for clip in track.clips.iter() {
                        let end = clip.end(self.state.bpm());
                        if end >= min_pos && clip.position < max_pos {
                            self.selected_clips.clip_ids.push(clip.id.clone());
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
                self.track_manager.get_track_y(
                    bounds.start_track_index,
                    response.rect,
                    &self.state,
                ),
            );

            let height = self
                .state
                .track_from_index(bounds.end_track_index)
                .map_or(0., |t| t.height);

            let max_point = Pos2::new(
                self.grid.beats_to_x(bounds.end_pos, response.rect),
                self.track_manager
                    .get_track_y(bounds.end_track_index, response.rect, &self.state)
                    + height,
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

    pub fn new(
        to_player_tx: Producer<GuiToPlayerMsg>,
        from_player_rx: Consumer<ProcessToGuiMsg>,
    ) -> Self {
        Self {
            clicked_pos: None,
            sample_preview: None,
            grid: WorkspaceGrid::new(),
            drag_state: None,
            selected_clips: Selection::new(),

            multiselect_start: None,
            track_manager: TrackManager::new(),

            y_offset: 0.,
            left_panel: UILeftPanel::new(),
            bottom_panel: UIBottomPanel::new(),
            top_bar: UITopBar::new(),

            state: ToniqueProjectState::new(to_player_tx, from_player_rx),
        }
    }
}
