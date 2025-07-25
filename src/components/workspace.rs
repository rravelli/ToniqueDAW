use crate::{
    ProcessToGuiMsg,
    analysis::AudioInfo,
    components::{
        clip::UIClip,
        grid::{MAX_RIGHT, MIN_LEFT, PIXEL_PER_BEAT, VIEW_WIDTH, WorkspaceGrid},
        track::{DEFAULT_TRACK_HEIGHT, HANDLE_HEIGHT, TrackSoloState, UITrack},
    },
    message::GuiToPlayerMsg,
    metrics::{AudioMetrics, GlobalMetrics},
};
use eframe::egui::{self, Sense, Stroke};
use egui::{Color32, Key, Layout, Painter, Pos2, Rect, Response, ScrollArea, StrokeKind, Ui, Vec2};
use rtrb::{Consumer, Producer};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaybackState {
    Paused,
    Playing,
}

const TOP_BAR_HEIGHT: f32 = 30.;
const LIMIT_WIDTH: f32 = 3.;

pub struct Workspace {
    // Communicate
    to_player_tx: Producer<GuiToPlayerMsg>,
    from_player_rx: Consumer<ProcessToGuiMsg>,
    // Global state
    pub bpm: f32,
    pub sample_rate: u32,
    pub playback_position: f32,
    // Navigation state
    grid: WorkspaceGrid,

    // Layout
    track_width: f32,
    multiselect_start: Option<Pos2>,

    // Tracks in the workspace
    tracks: Vec<UITrack>,
    master_track: UITrack,
    sample_preview: Option<UIClip>,

    drag_state: Option<DragState>,

    selected_clips: Vec<String>,
    selected_track: Option<String>,

    playback_state: PlaybackState,

    metrics: GlobalMetrics,
    solo_tracks: Vec<String>,

    y_offset: f32,
}

struct DragState {
    element: UIClip,
    mouse_origin: Pos2,
}

impl Workspace {
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        dragged_audio_info: Option<AudioInfo>,
        is_released: bool,
    ) {
        while let Ok(msg) = self.from_player_rx.pop() {
            match msg {
                ProcessToGuiMsg::PlaybackPos(pos) => {
                    self.playback_position =
                        self.bpm * (pos as f32) / (self.sample_rate as f32 * 60.);

                    self.playback_state = PlaybackState::Playing;
                }
                ProcessToGuiMsg::Metrics(metrics) => self.metrics = metrics,
            }
        }

        self.watch_inputs(ui);

        if self.playback_state == PlaybackState::Playing {
            ui.ctx().request_repaint();
        }

        if ui.input(|i| i.key_pressed(egui::Key::Space)) {
            if self.playback_state == PlaybackState::Playing {
                let _ = self.to_player_tx.push(GuiToPlayerMsg::Pause);
                self.playback_state = PlaybackState::Paused
            } else {
                let _ = self.to_player_tx.push(GuiToPlayerMsg::Play);
                self.playback_state = PlaybackState::Playing;
            }
        }

        ui.vertical(|ui| {
            self.navigation_bar(ui);

            let scroll = ScrollArea::vertical()
                .id_salt("workspace-scrollbar")
                .max_height(ui.available_height())
                .show(ui, |ui| {
                    let mut height = 100.;
                    for track in self.tracks.iter() {
                        height += track.height + HANDLE_HEIGHT;
                    }

                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), height.max(ui.available_height())),
                        Layout::left_to_right(egui::Align::Min),
                        |ui| {
                            // Grid area
                            let (response, painter) = ui.allocate_painter(
                                egui::Vec2::new(
                                    ui.available_width() - self.track_width - LIMIT_WIDTH,
                                    ui.available_height(),
                                ),
                                Sense::click_and_drag(),
                            );

                            let rect = response.rect;

                            if response.clicked() {
                                self.selected_clips = vec![];
                                self.selected_track = None;
                            }
                            let scroll_delta = ui.input(|i| i.smooth_scroll_delta.x);

                            if scroll_delta != 0.
                                && let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos())
                                && rect.contains(mouse_pos)
                            {
                                self.grid.scroll(-scroll_delta);
                            }

                            if response.double_clicked()
                                && let Some(mouse_pos) = ui.ctx().input(|i| i.pointer.hover_pos())
                            {
                                self.playback_position = self.grid.x_to_beats(mouse_pos.x, rect);
                                let _ = self
                                    .to_player_tx
                                    .push(GuiToPlayerMsg::SeekTo(self.playback_position));
                            }
                            // Do not draw after clips
                            self.resize_handle(ui);
                            self.track_panel(ui);
                            // Draw grid & samples
                            self.grid.paint(&painter, rect);

                            self.paint_clips(ui, rect);
                            self.paint_tracks(&painter, rect);
                            self.paint_preview_sample(ui, rect, dragged_audio_info, is_released);
                            self.paint_playback_cursor(&painter, rect);
                            self.handle_multiselect(ui, response);
                            self.scrollbar(rect, &painter, ui);
                        },
                    )
                });
            self.y_offset = scroll.state.offset.y;

            ui.allocate_ui_at_rect(
                Rect::from_min_size(
                    Pos2::new(
                        scroll.inner_rect.left(),
                        scroll.inner_rect.bottom() - self.master_track.height,
                    ),
                    Vec2::new(scroll.inner_rect.width(), self.master_track.height),
                ),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(ui.available_width() - self.track_width);
                        self.master_track.ui(
                            ui,
                            self.metrics.master.clone(),
                            TrackSoloState::NotSoloing,
                            false,
                        );
                    })
                },
            )
        });
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
            self.track_width -= response.drag_delta().x;
            self.track_width = self.track_width.clamp(30., 300.);
        }
    }

    // Tracks
    fn track_panel(&mut self, ui: &mut egui::Ui) {
        let res = ui.vertical(|ui| {
            ui.set_width(self.track_width);
            // ui.add_space(TOP_BAR_HEIGHT);
            for track in self.tracks.iter_mut() {
                let metrics = self.metrics.tracks.get(&track.id);
                let metrics = if let Some(metrics) = metrics {
                    metrics.value().clone()
                } else {
                    AudioMetrics::new()
                };
                let solo = if self.solo_tracks.is_empty() {
                    TrackSoloState::NotSoloing
                } else if self.solo_tracks.contains(&track.id) {
                    TrackSoloState::Solo
                } else {
                    TrackSoloState::Soloing
                };
                let selected = self.selected_track.clone().is_some_and(|id| id == track.id);
                // Render track
                let (mute_changed, volume_changed, solo_clicked, clicked) =
                    track.ui(ui, metrics, solo, selected);
                // Track events
                if mute_changed {
                    let _ = self
                        .to_player_tx
                        .push(GuiToPlayerMsg::MuteTrack(track.id.clone(), track.muted));
                };
                if volume_changed {
                    let _ = self.to_player_tx.push(GuiToPlayerMsg::ChangeTrackVolume(
                        track.id.clone(),
                        track.volume,
                    ));
                }
                if solo_clicked {
                    self.solo_tracks = if self.solo_tracks.contains(&track.id) {
                        vec![]
                    } else {
                        vec![track.id.clone()]
                    };
                    let _ = self
                        .to_player_tx
                        .push(GuiToPlayerMsg::SoloTracks(self.solo_tracks.clone()));
                }
                if clicked {
                    if selected {
                        self.selected_track = None;
                    } else {
                        self.selected_track = Some(track.id.clone());
                    }
                }
            }
        });

        // context menu
        ui.interact(
            Rect::from_min_size(
                res.response.rect.left_bottom(),
                Vec2::new(self.track_width, ui.available_height()),
            ),
            "tracks".into(),
            Sense::click(),
        )
        .context_menu(|ui| {
            if ui.button("Add track").clicked() {
                let name = format!("Track {}", self.tracks.len() + 1).to_string();
                self.create_track(name.as_str());
                ui.close();
            }
        });
    }

    fn paint_clips(&mut self, ui: &mut Ui, viewport: egui::Rect) {
        let mut y = viewport.top();
        let mut dragged_track_index = None;
        let mut dragged_clip_index = None;

        for (track_index, track) in self.tracks.iter_mut().enumerate() {
            let height = track.clone().height;
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
                            self.selected_clips.contains(&clip.id()),
                            &mut self.to_player_tx,
                        );

                        if response.clicked() {
                            if self.selected_clips.contains(&clip.id()) {
                                self.selected_clips = vec![];
                            } else {
                                self.selected_clips = vec![clip.id()];
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
            let clip = self.tracks[old_track].clips.remove(s);
            let (_, y) = self.find_track_at(viewport, mouse_pos.y);
            let x = self.grid.beats_to_x(clip.position, viewport);
            self.drag_state = Some(DragState {
                element: clip,
                mouse_origin: Pos2::new(mouse_pos.x - x, mouse_pos.y - y),
            })
        }
        // render draggin objects
        if let Some(mut drag_state) = self.drag_state.take()
            && let Some(duration) = drag_state.element.duration()
            && let Some(mouse_pos) = mouse_pos
        {
            let (t, y) = self.find_track_at(viewport, mouse_pos.y);
            let width = self.grid.duration_to_width(duration, self.bpm);
            let new_position = self
                .grid
                .x_to_beats(mouse_pos.x - drag_state.mouse_origin.x, viewport);
            let snapped_position = self.grid.snap_at_grid(new_position);

            let x = self.grid.beats_to_x(snapped_position, viewport);

            drag_state.element.position = snapped_position;
            let _ = drag_state.element.ui(
                ui,
                Pos2::new(x, y),
                Vec2::new(
                    width,
                    if let Some(t) = t {
                        self.tracks[t].height
                    } else {
                        DEFAULT_TRACK_HEIGHT
                    },
                ),
                viewport,
                &self.grid,
                self.bpm,
                "".to_string(),
                false,
                &mut self.to_player_tx,
            );
            // clip released
            if !ui.input(|i| i.pointer.primary_down()) {
                let clone = drag_state.element.clone();
                let track_index = if let Some(track_index) = t {
                    track_index
                } else {
                    self.create_track("Audio Track");
                    self.tracks.len() - 1
                };

                self.tracks[track_index].add_clip(clone, self.bpm, &mut self.to_player_tx);

                let _ = self.to_player_tx.push(GuiToPlayerMsg::MoveClip(
                    drag_state.element.id(),
                    self.tracks[track_index].clone().id,
                    drag_state.element.position,
                ));
                self.drag_state = None;
            } else {
                self.drag_state = Some(drag_state);
            }
        }
    }

    fn watch_inputs(&mut self, ui: &mut Ui) {
        let mut new_selected = vec![];
        let mut updated = false;

        if ui.input(|i| i.key_pressed(Key::D) && i.modifiers.ctrl) {
            // Duplicate clips
            for id in self.selected_clips.iter() {
                for track in self.tracks.iter_mut() {
                    let clip = track.clips.iter().find(|c| c.id() == *id);
                    if let Some(clip) = clip {
                        let mut duplicated_clip = clip.clone_with_new_id();
                        duplicated_clip.position = clip.end(self.bpm);
                        new_selected.push(duplicated_clip.id().clone());
                        let _ = self.to_player_tx.push(GuiToPlayerMsg::AddClip(
                            track.id.clone(),
                            duplicated_clip.audio.path.clone(),
                            duplicated_clip.position,
                            duplicated_clip.id(),
                            duplicated_clip.trim_start,
                            duplicated_clip.trim_end,
                        ));
                        track.add_clip(duplicated_clip, self.bpm, &mut self.to_player_tx);

                        updated = true;
                        break;
                    }
                }
            }
        } else if ui
            .input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
        {
            // Delete
            if let Some(track_id) = self.selected_track.take() {
                self.delete_track(track_id);
            } else {
                for track in self.tracks.iter_mut() {
                    track.delete_ids(self.selected_clips.clone());
                    for id in self.selected_clips.clone() {
                        let _ = self
                            .to_player_tx
                            .push(GuiToPlayerMsg::RemoveClip(id, track.id.clone()));
                    }
                }
            };
        } else if ui.input(|i| i.key_pressed(Key::K) && i.modifiers.ctrl) {
            // Cut clips
            let mut added_clip = vec![];

            for (index, track) in self.tracks.iter_mut().enumerate() {
                for clip in track.clips.iter_mut() {
                    if clip.position < self.playback_position
                        && self.playback_position < clip.end(self.bpm)
                        && (self.selected_clips.is_empty()
                            || self.selected_clips.contains(&clip.id()))
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
                    clip.id(),
                    clip.audio.path.clone(),
                    clip.position,
                    self.tracks[track].id.clone(),
                    clip.trim_start,
                    clip.trim_end,
                ));
                self.tracks[track].clips.push(clip);
            }
        }

        if updated {
            self.selected_clips = new_selected;
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
            let snapped_position = self.grid.snap_at_grid(position);
            // Create preview if not done yet
            if self.sample_preview.is_none() {
                self.sample_preview = Some(UIClip::new(audio_info, snapped_position));
            }

            let mouse_y = mouse_pos.y;

            // Find corresponding track
            let (track_index, track_y) = self.find_track_at(rect, mouse_y);
            let height = if let Some(t) = track_index {
                self.tracks[t].height
            } else {
                DEFAULT_TRACK_HEIGHT
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
                );
            }
        };

        if is_released
            && let Some(mouse_pos) = ui.ctx().input(|i| i.pointer.hover_pos())
            && let Some(preview) = self.sample_preview.clone()
        {
            // remove preview
            self.sample_preview = None;
            // add sample
            if rect.contains(mouse_pos) {
                let (track_index, _) = self.find_track_at(rect, mouse_pos.y);
                let index;
                if let Some(track_index) = track_index {
                    index = track_index;
                } else {
                    // Create new track
                    self.create_track("Audio Track");
                    index = self.tracks.len() - 1;
                }
                let id = preview.id();
                let _ = self.to_player_tx.push(GuiToPlayerMsg::AddClip(
                    self.tracks[index].id.clone(),
                    preview.audio.path.clone(),
                    preview.position,
                    id,
                    preview.trim_start,
                    preview.trim_end,
                ));
                self.tracks[index].add_clip(preview, self.bpm, &mut self.to_player_tx);
            }
        }
    }

    fn create_track(&mut self, name: &str) {
        let id = uuid::Uuid::new_v4().to_string();
        self.tracks.push(UITrack::new(&id, name));
        let _ = self.to_player_tx.push(GuiToPlayerMsg::AddTrack(id));
    }

    fn delete_track(&mut self, id: String) {
        for (i, track) in self.tracks.iter().enumerate() {
            if track.id == id {
                self.tracks.remove(i);
                let _ = self.to_player_tx.push(GuiToPlayerMsg::RemoveTrack(id));
                break;
            }
        }
    }

    fn paint_tracks(&self, painter: &Painter, rect: Rect) {
        let mut y = rect.top();
        let solo = !self.solo_tracks.is_empty();
        for track in self.tracks.iter() {
            if !self.solo_tracks.contains(&track.id) && (track.muted || solo) {
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(rect.left(), y),
                        Pos2::new(rect.right(), y + track.height),
                    ),
                    0.,
                    Color32::from_black_alpha(70),
                );
            }
            y += track.height;
            painter.line(
                vec![
                    Pos2::new(rect.left(), y + HANDLE_HEIGHT / 2.),
                    Pos2::new(rect.right(), y + HANDLE_HEIGHT / 2.),
                ],
                Stroke::new(HANDLE_HEIGHT, Color32::from_gray(40)),
            );
            y += HANDLE_HEIGHT
        }
    }

    fn find_track_at(&self, rect: Rect, y_pos: f32) -> (Option<usize>, f32) {
        // y position of the top of the track
        let mut track_y = rect.top();
        let mut index = None;
        for (i, track) in self.tracks.iter().enumerate() {
            if y_pos <= track_y + track.height {
                index = Some(i);
                break;
            }
            track_y += track.height + HANDLE_HEIGHT;
        }

        (index, track_y)
    }

    fn navigation_bar(&mut self, ui: &mut Ui) {
        // Rectangle for zoom control
        let zoom_rect = egui::Rect::from_min_size(
            egui::pos2(ui.min_rect().left(), ui.min_rect().top()),
            egui::vec2(
                ui.available_width() - self.track_width - LIMIT_WIDTH,
                TOP_BAR_HEIGHT,
            ),
        );
        let zoom_response = ui.allocate_rect(zoom_rect, Sense::click_and_drag());

        // Draw the zoom control rectangle
        ui.painter()
            .rect_filled(zoom_rect, 0.0, egui::Color32::from_gray(80));

        self.draw_labels(ui.painter(), zoom_rect);

        // Change cursor to resize vertical
        if zoom_response.hovered() || zoom_response.dragged() {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeVertical);
        }

        // Handle mouse drag to change zoom, centered at mouse x
        if zoom_response.dragged() {
            // Mouse position relative to zoom_rect
            let mouse_x = zoom_response
                .interact_pointer_pos()
                .map(|pos| pos.x - zoom_rect.left())
                .unwrap_or(VIEW_WIDTH / 2.0);

            self.grid
                .zoom_and_drag_at(mouse_x, zoom_response.drag_delta());
        }
    }

    fn draw_labels(&self, painter: &egui::Painter, rect: egui::Rect) {
        let delta = self.grid.right - self.grid.left;
        let grid_step = PIXEL_PER_BEAT * VIEW_WIDTH / delta as f32; // 10 pixels per grid line
        let grid_color = egui::Color32::from_gray(90);

        // Find the first grid line to draw (leftmost visible)
        let start_x = rect.left() - (self.grid.left as f32 * VIEW_WIDTH / delta as f32 % grid_step);
        let mut x = start_x;
        let mut beat = (self.grid.left / PIXEL_PER_BEAT) as usize;

        while x < rect.right() {
            if delta < 200. || (delta < 1500. && beat % 4 == 0) || beat % 16 == 0 {
                painter.line_segment(
                    [
                        egui::Pos2::new(x, rect.bottom() - 6.0),
                        egui::Pos2::new(x, rect.bottom() - 2.0),
                    ],
                    Stroke::new(2., grid_color),
                );
                let bar = beat.div_euclid(4);
                let sub_beat = beat % 4;

                let text = if sub_beat == 0 {
                    format!("{}", bar + 1)
                } else {
                    format!("{}.{}", bar + 1, sub_beat + 1)
                };

                painter.text(
                    egui::Pos2::new(x, rect.bottom() - 8.0),
                    egui::Align2::CENTER_BOTTOM,
                    text,
                    egui::FontId::new(8.0, egui::FontFamily::Monospace),
                    egui::Color32::WHITE,
                );
            }
            x += grid_step;
            beat += 1;
        }
    }

    fn handle_multiselect(&mut self, ui: &mut Ui, response: Response) {
        if response.drag_started()
            && let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos())
        {
            self.multiselect_start = Some(mouse_pos);
        }
        if response.drag_stopped() {
            self.multiselect_start = None;
        }

        if let Some(start) = self.multiselect_start
            && let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos())
        {
            let select_rect = Rect::from_points(&[start, mouse_pos]);
            ui.painter().rect_stroke(
                select_rect,
                0.,
                Stroke::new(1. / ui.pixels_per_point(), Color32::from_white_alpha(250)),
                StrokeKind::Inside,
            );
            self.selected_clips = vec![];
            for tracks in self.tracks.iter() {
                for clip in tracks.clips.iter() {
                    let start_x = self.grid.beats_to_x(clip.position, response.rect);
                    let end_x = self.grid.beats_to_x(clip.end(self.bpm), response.rect);
                    if (select_rect.min.x <= start_x && start_x <= select_rect.max.x)
                        || (select_rect.min.x <= end_x && end_x <= select_rect.max.x)
                    {
                        self.selected_clips.push(clip.id());
                    }
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
            to_player_tx,
            bpm: 120.0,
            sample_rate: 44100,
            tracks: vec![],
            track_width: 140.,
            sample_preview: None,
            grid: WorkspaceGrid::new(),
            drag_state: None,
            selected_clips: vec![],
            playback_position: 0.,
            playback_state: PlaybackState::Paused,
            metrics: GlobalMetrics::new(),
            multiselect_start: None,
            master_track: UITrack::new("master", "Master"),
            solo_tracks: vec![],
            selected_track: None,
            y_offset: 0.,
        }
    }
}
