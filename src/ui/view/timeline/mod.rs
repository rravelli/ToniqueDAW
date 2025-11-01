use crate::{
    analysis::AudioInfo,
    cache::AUDIO_ANALYSIS_CACHE,
    core::{
        clip::ClipCore,
        state::ToniqueProjectState,
        track::{DEFAULT_TRACK_HEIGHT, TrackCore, TrackReferenceCore},
    },
    ui::{
        clip::UIClip,
        panels::left_panel::DragPayload,
        theme::PRIMARY_COLOR,
        track::HANDLE_HEIGHT,
        utils::find_track_at,
        view::timeline::{
            drag::DragState,
            selection::{ClipSelection, Multiselect},
        },
    },
};
use egui::{Color32, DragAndDrop, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2};
mod drag;
mod keys;
mod selection;

pub struct UITimeline {
    // TODO: Merge into state
    selected_clips: ClipSelection,
    drag_state: Option<DragState>,
    clicked_pos: Option<Pos2>,
    multiselect_start: Option<Multiselect>,
}

impl UITimeline {
    pub fn new() -> Self {
        Self {
            selected_clips: ClipSelection::new(),
            drag_state: None,
            clicked_pos: None,
            multiselect_start: None,
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        state: &mut ToniqueProjectState,
        viewport: Rect,
        offset: Vec2,
    ) {
        // First handle key presses
        self.handle_key_press(ui, state);
        // Create timeline area
        let timeline_res = ui.allocate_rect(viewport, Sense::all());
        let painter = ui.painter_at(viewport);
        // Handle interactions in the timeline area
        self.interact(ui, &timeline_res, state, viewport);

        // Rendering
        // First render the grid
        state.grid.render_grid(&painter, viewport);
        // Render all clips (except dragged clips)
        self.render_clips(ui, state, viewport, offset);

        let (audio, is_released) = self.dnd(&timeline_res);

        if let Some(audio) = audio {
            self.render_preview_clip(ui, viewport, offset, audio, is_released, state);
        }

        self.handle_dropped_audio(ui, viewport, state);

        // Draw multiselect zone
        self.handle_multiselect(ui, state, &timeline_res);

        let hovered_files = ui.input(|i| i.raw.hovered_files.clone());
        if !hovered_files.is_empty() {
            painter.rect_filled(viewport, 1.0, PRIMARY_COLOR.gamma_multiply_u8(20));
        }
    }

    pub fn interact(
        &mut self,
        ui: &mut Ui,
        response: &Response,
        state: &mut ToniqueProjectState,
        viewport: Rect,
    ) {
        if response.clicked()
            && let Some(mouse_pos) = response.interact_pointer_pos()
        {
            self.selected_clips.reset();
            state.pause_preview();
            state.set_playback_position(state.grid.x_to_beats(mouse_pos.x, viewport));
        }
        if let Some(mouse_pos) = response.hover_pos()
            && ui.input(|i| i.smooth_scroll_delta.y != 0. && i.modifiers.alt)
        {
            let delta = ui.input(|i| i.smooth_scroll_delta.y);
            state.grid.zoom_around(delta, mouse_pos.x, viewport);
        }
    }

    pub fn render_clips(
        &mut self,
        ui: &mut Ui,
        state: &mut ToniqueProjectState,
        viewport: Rect,
        offset: Vec2,
    ) {
        let mut y = viewport.top();
        let tracks: Vec<_> = state.tracks().collect();
        let mut dragged_track_index = None;
        let mut dragged_clip = None;
        let dragged_ids = self
            .drag_state
            .as_ref()
            .map_or(Vec::new(), |d| d.dragged_ids());

        for track in tracks {
            let track_bottom = y + track.height;
            let view_top = viewport.top() + offset.y;
            let view_bottom = view_top + viewport.height();

            // Skip if track is entirely outside the visible vertical range
            if track_bottom < view_top || y > view_bottom {
                y += track.height + HANDLE_HEIGHT;
                continue;
            }

            let track_rect = Rect::from_min_max(
                pos2(viewport.left(), y - offset.y),
                pos2(viewport.right(), y + track.height - offset.y),
            );

            if ui.input(|i| {
                i.pointer.primary_pressed()
                    && i.pointer
                        .hover_pos()
                        .is_some_and(|p| track_rect.contains(p))
            }) {
                state.select_track(&track.id);
            }

            if track.selected {
                ui.painter()
                    .rect_filled(track_rect, 1.0, track.color.gamma_multiply_u8(10));
            }

            for mut clip in track.clips.clone() {
                if let Some((id, start, end, pos)) = &state.resized_clip
                    && clip.id == *id
                {
                    clip.trim_start = *start;
                    clip.trim_end = *end;
                    clip.position = *pos;
                }
                let dragged = self.render_clip(
                    &track,
                    &clip,
                    ui,
                    state,
                    viewport,
                    offset,
                    y,
                    dragged_ids.contains(&clip.id),
                );
                if dragged_clip.is_none() && dragged {
                    dragged_clip = Some(clip.clone());
                    dragged_track_index = Some(track.index)
                }
            }

            self.handle_track_hover(ui, state, &track, track_rect);

            y += track.height;
            self.paint_track_separator(ui, viewport, offset, y);
            y += HANDLE_HEIGHT;
        }

        self.handle_dragged_clips(ui, dragged_track_index, viewport, dragged_clip, state);
    }

    fn handle_track_hover(
        &mut self,
        ui: &mut Ui,
        state: &mut ToniqueProjectState,
        track: &TrackReferenceCore,
        track_rect: Rect,
    ) {
        if let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos())
            && track_rect.contains(mouse_pos)
            && let Some(payload) = DragAndDrop::payload::<DragPayload>(ui.ctx())
            && let DragPayload::Effect(_) = *payload
        {
            ui.painter()
                .rect_filled(track_rect, 1.0, Color32::from_white_alpha(60));
        }

        if let Some(pointer) = ui.input(|r| r.pointer.hover_pos())
            && ui.input(|i| i.pointer.any_released())
            && track_rect.contains(pointer)
            && let Some(payload) = DragAndDrop::payload::<DragPayload>(ui.ctx())
            && let DragPayload::Effect(id) = *payload
        {
            state.add_effect(&track.id, id, 0);
            state.select_track(&track.id);
            state.bottom_panel_open = true;
            DragAndDrop::take_payload::<DragPayload>(ui.ctx());
        }
    }

    fn render_clip(
        &mut self,
        track: &TrackReferenceCore,
        clip: &ClipCore,
        ui: &mut Ui,
        state: &mut ToniqueProjectState,
        viewport: Rect,
        offset: Vec2,
        y: f32,
        dragged: bool,
    ) -> bool {
        let x = state.grid.beats_to_x(clip.position, viewport);
        let width = state
            .grid
            .duration_to_width(clip.duration().unwrap(), state.bpm());

        if x + width < viewport.left() || x > viewport.right() {
            return false;
        }
        let pos = pos2(x, y - offset.y);
        let size = vec2(width, track.height);
        let color = if track.disabled() {
            Color32::from_gray(100)
        } else if dragged {
            Color32::from_white_alpha(10)
        } else {
            track.color
        };
        let response = UIClip::new().ui(
            ui,
            pos,
            size,
            viewport,
            !dragged && self.selected_clips.clip_ids.contains(&clip.id),
            &clip,
            state,
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
                state.select_track(&track.id);
            }
        }

        response.dragged()
    }

    fn paint_track_separator(&self, ui: &mut Ui, viewport: Rect, offset: Vec2, y: f32) {
        let painter = ui.painter_at(viewport);
        painter.line(
            vec![
                pos2(viewport.left(), y + HANDLE_HEIGHT / 2. - offset.y),
                pos2(viewport.right(), y + HANDLE_HEIGHT / 2. - offset.y),
            ],
            Stroke::new(HANDLE_HEIGHT, Color32::from_gray(60)),
        );
    }

    pub fn dnd(&mut self, response: &Response) -> (Option<AudioInfo>, bool) {
        let mut dragged_audio = None;
        let mut is_released = false;
        if let Some(payload) = response.dnd_hover_payload::<DragPayload>()
            && let DragPayload::File(audio) = payload.as_ref()
        {
            dragged_audio = Some(audio.clone());
            if let Some(payload) = response.dnd_release_payload::<DragPayload>()
                && let DragPayload::File(audio) = payload.as_ref()
            {
                dragged_audio = Some(audio.clone());
                is_released = true;
            }
        }

        (dragged_audio, is_released)
    }

    fn handle_dropped_audio(
        &mut self,
        ui: &mut Ui,
        viewport: Rect,
        state: &mut ToniqueProjectState,
    ) {
        let dropped_files = ui.input(|i| i.raw.dropped_files.clone());
        state.begin_batch();
        if !dropped_files.is_empty() {
            for file in dropped_files {
                if let Some(path) = file.path {
                    if let Some(audio_info) = AUDIO_ANALYSIS_CACHE.get_or_analyze(path) {
                        if let Some(mouse_pos) = ui.ctx().input(|i| i.pointer.hover_pos())
                            && viewport.contains(mouse_pos)
                        {
                            // Convert x to beats and snap to grid
                            let position = state.grid.x_to_beats(mouse_pos.x, viewport);
                            let snapped_position = state.grid.snap_at_grid(position);

                            let new_track = TrackCore::new();
                            state.add_track(new_track.clone());
                            let clip = ClipCore::new(audio_info, snapped_position);
                            state.add_clips(&new_track.id, vec![clip]);
                        }
                    }
                }
            }
        }
        state.commit_batch();
    }

    fn render_preview_clip(
        &mut self,
        ui: &mut Ui,
        viewport: egui::Rect,
        offset: Vec2,
        audio_info: AudioInfo,
        is_released: bool,
        state: &mut ToniqueProjectState,
    ) {
        // Render preview clip
        if let Some(duration) = audio_info.duration
            && let Some(mouse_pos) = ui.ctx().input(|i| i.pointer.hover_pos())
            && viewport.contains(mouse_pos)
        {
            // Calculate grid position in beats
            let position = state.grid.x_to_beats(mouse_pos.x, viewport);
            let snapped_position = state.grid.snap_at_grid(position);
            let x = state.grid.beats_to_x(snapped_position, viewport);
            let mouse_y = mouse_pos.y;

            // Find corresponding track
            let (track, y) = find_track_at(state, viewport, mouse_y);

            let height = track.as_ref().map_or(DEFAULT_TRACK_HEIGHT, |t| t.height);
            let show_waveform = track.as_ref().map_or(true, |t| !t.closed);
            let color = track.as_ref().map_or(Color32::WHITE, |t| t.color);
            let width = state.grid.duration_to_width(duration, state.bpm());

            let pos = pos2(x, y - offset.y);
            let size = Vec2::new(width, height);
            let clip = ClipCore::new(audio_info, snapped_position);
            // render clip
            UIClip::new().ui(
                ui,
                pos,
                size,
                viewport,
                false,
                &clip,
                state,
                show_waveform,
                color,
            );

            if is_released {
                state.begin_batch();
                let id = if let Some(t) = track {
                    t.id
                } else {
                    let new_track = TrackCore::new();
                    state.add_track(new_track.clone());
                    new_track.id
                };
                state.add_clips(&id, vec![clip]);
                state.commit_batch();
            }
        };
    }
}
