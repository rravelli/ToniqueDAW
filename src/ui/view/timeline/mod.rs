use crate::{
    analysis::AudioInfo,
    cache::AUDIO_ANALYSIS_CACHE,
    core::{
        clip::ClipCore,
        state::ToniqueProjectState,
        track::{DEFAULT_TRACK_HEIGHT, TRACK_CLOSED_HEIGHT, TrackKind, TrackReferenceCore},
    },
    ui::{
        clip::UIClip,
        panels::left_panel::DragPayload,
        theme::{ThemeExt, with_alpha},
        track::HANDLE_HEIGHT,
        utils::find_track_at,
        view::timeline::{drag::DragState, selection::Multiselect},
    },
};
use egui::{DragAndDrop, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2};
use tonique_engine::edit::{ClipId, TrackId};
mod drag;
mod keys;
mod scroll;
mod selection;

/// Opacity of the clips in a group's preview, out of 255.
const PREVIEW_ALPHA: u8 = 80;

pub struct UITimeline {
    drag_state: Option<DragState>,
    clicked_pos: Option<Pos2>,
    multiselect_start: Option<Multiselect>,
    /// Folded groups opened by dragging over them, to fold again once left.
    hover_unfolded: Vec<TrackId>,
}

impl UITimeline {
    pub fn new() -> Self {
        Self {
            drag_state: None,
            clicked_pos: None,
            multiselect_start: None,
            hover_unfolded: Vec::new(),
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
        self.handle_key_press(ui, state, viewport);
        // Create timeline area
        let timeline_res = ui.allocate_rect(viewport, Sense::all());
        let painter = ui.painter_at(viewport);
        // Handle interactions in the timeline area
        self.interact(ui, &timeline_res, state, viewport);

        // Rendering
        // First render the grid
        state.grid.render_grid(&painter, viewport, &ui.app_theme());
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
            painter.rect_filled(viewport, 1.0, with_alpha(ui.app_theme().accent, 20));
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
            state.clear_clip_selection();
            let beats = state.grid.x_to_beats(mouse_pos.x, viewport);
            state.set_edit_cursor(state.grid.snap_at_grid(beats));
        }
        // Ctrl+wheel (and pinch) zooms; egui reports it as a zoom factor.
        let zoom = ui.input(|i| i.zoom_delta());
        if zoom != 1.
            && let Some(mouse_pos) = response.hover_pos()
        {
            state.grid.zoom_by(zoom, mouse_pos.x, viewport);
        }
    }

    /// While audio or clips are dragged, open the folded group under the
    /// pointer, and fold it again once the pointer leaves it. Dropping inside
    /// leaves it open, to show where things landed.
    fn unfold_on_hover(
        &mut self,
        ui: &Ui,
        state: &mut ToniqueProjectState,
        rows: &[TrackReferenceCore],
        viewport: Rect,
        offset: Vec2,
    ) {
        let dragging = self.drag_state.is_some()
            || DragAndDrop::payload::<DragPayload>(ui.ctx())
                .is_some_and(|p| matches!(*p, DragPayload::File(_)));
        let pointer = ui.input(|i| i.pointer.hover_pos());

        // Each row's vertical span on screen.
        let mut y = viewport.top() - offset.y;
        let spans: Vec<(&TrackReferenceCore, f32, f32)> = rows
            .iter()
            .map(|row| {
                let span = (row, y, y + row.height);
                y += row.height + HANDLE_HEIGHT;
                span
            })
            .collect();
        // A group's span runs to the end of its last row.
        let scope = |id: TrackId| {
            let i = spans.iter().position(|(r, _, _)| r.id == id)?;
            let (group, top, bottom) = spans[i];
            let end = spans[i + 1..]
                .iter()
                .take_while(|(r, _, _)| r.depth > group.depth)
                .last()
                .map_or(bottom, |(_, _, b)| *b);
            Some(top..=end + HANDLE_HEIGHT)
        };

        if dragging && let Some(p) = pointer.filter(|p| viewport.contains(*p)) {
            for (row, top, bottom) in &spans {
                if row.kind == TrackKind::Group && row.closed && (*top..=*bottom).contains(&p.y) {
                    state.set_closed(&row.id, false);
                    self.hover_unfolded.push(row.id);
                }
            }
        }
        self.hover_unfolded.retain(|group| {
            let inside = pointer
                .zip(scope(*group))
                .is_some_and(|(p, span)| span.contains(&p.y));
            match (inside, dragging) {
                // Still hovering it.
                (true, true) => true,
                // Dropped inside: stays open.
                (true, false) => false,
                // Left it: fold again.
                (false, _) => {
                    state.set_closed(group, true);
                    false
                }
            }
        });
    }

    pub fn render_clips(
        &mut self,
        ui: &mut Ui,
        state: &mut ToniqueProjectState,
        viewport: Rect,
        offset: Vec2,
    ) {
        let tracks = state.rows();
        self.unfold_on_hover(ui, state, &tracks, viewport, offset);
        let mut y = viewport.top();
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

            // A group's lane: an overview of everything inside.
            let clips = if track.kind == TrackKind::Group {
                paint_group_overview(ui, state, &track, track_rect);
                Vec::new()
            } else {
                track.clips.clone()
            };
            for mut clip in clips {
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
                .rect_filled(track_rect, 1.0, ui.app_theme().hover_overlay);
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

        let top = y - offset.y;
        if x + width < viewport.left()
            || x > viewport.right()
            || top + track.height < viewport.top()
            || top > viewport.bottom()
        {
            return false;
        }
        let pos = pos2(x, top);
        let size = vec2(width, track.height);
        let theme = ui.app_theme();
        let color = if track.disabled() {
            theme.bg_control_hover
        } else if dragged {
            theme.hover_overlay
        } else {
            track.color
        };
        let response = UIClip::new().ui(
            ui,
            pos,
            size,
            viewport,
            !dragged && state.is_clip_selected(clip.id),
            &clip,
            state,
            !track.closed,
            color,
        );
        // Select clip
        // Shift-click adds or removes the clip; a plain click selects it alone.
        if response.clicked() {
            if ui.input(|r| r.modifiers.shift) {
                state.toggle_clip_selected(clip.id);
            } else {
                state.select_clips(vec![clip.id]);
            }
            state.select_track(&track.id);
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
            Stroke::new(HANDLE_HEIGHT, ui.app_theme().separator),
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
                if let Some(audio_info) =
                    AUDIO_ANALYSIS_CACHE.get_or_analyze(file.path().to_path_buf())
                    && let Some(mouse_pos) = ui.ctx().input(|i| i.pointer.hover_pos())
                    && viewport.contains(mouse_pos)
                {
                    // Convert x to beats and snap to grid
                    let position = state.grid.x_to_beats(mouse_pos.x, viewport);
                    let snapped_position = state.grid.snap_at_grid(position);

                    let track = state.add_track();
                    let clip = ClipCore::new(state.new_clip_id(), audio_info, snapped_position);
                    state.add_clips(&track, vec![clip]);
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
            let color = track
                .as_ref()
                .map_or(ui.app_theme().text_muted, |t| t.color);
            let width = state.grid.duration_to_width(duration, state.bpm());

            let pos = pos2(x, y - offset.y);
            let size = Vec2::new(width, height);
            // Placeholder ID: the clip only gets a real one when dropped
            let clip = ClipCore::new(ClipId(0), audio_info, snapped_position);
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
                let id = match track {
                    Some(t) => t.id,
                    None => state.add_track(),
                };
                let clip = clip.with_id(state.new_clip_id());
                state.add_clips(&id, vec![clip]);
                state.commit_batch();
            }
        };
    }
}

/// A group's content, in a band the height of a folded group at the top of
/// its lane: a strip per track inside it, in order, with that track's clips
/// in its colour, faint so they don't pass for real clips.
fn paint_group_overview(
    ui: &Ui,
    state: &ToniqueProjectState,
    group: &TrackReferenceCore,
    lane: Rect,
) {
    let tracks = state.group_tracks(group.id);
    if tracks.is_empty() {
        return;
    }
    let theme = ui.app_theme();
    let painter = ui.painter_at(lane);
    let bpm = state.bpm();
    let band = Rect::from_min_max(
        lane.min,
        pos2(
            lane.right(),
            (lane.top() + TRACK_CLOSED_HEIGHT).min(lane.bottom()),
        ),
    )
    .shrink2(vec2(0., 2.));
    let strip = band.height() / tracks.len() as f32;
    for (k, track) in tracks.iter().enumerate() {
        let top = band.top() + k as f32 * strip;
        let rows = top..=top + strip;
        let color = if track.disabled() {
            theme.text_disabled
        } else {
            track.color
        };
        let color = with_alpha(color, PREVIEW_ALPHA);
        for clip in &track.clips {
            let x = state.grid.beats_to_x(clip.position, lane)
                ..=state.grid.beats_to_x(clip.end(bpm), lane);
            painter.rect_filled(Rect::from_x_y_ranges(x, rows.clone()), 1., color);
        }
    }
}
