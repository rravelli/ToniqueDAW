use crate::ui::RectSpan;
use crate::ui::theme::ToEgui;
use crate::ui::{
    arrangement::clip::ClipView,
    arrangement::{
        row_layout::RowLayout,
        timeline::{Timeline, scroll::autoscroll},
    },
    theme::ThemeExt,
};
use egui::{Rect, Stroke, Ui, pos2, vec2};
use tonique_core::session::timeline_view::TimelineView;
use tonique_core::{
    document::track::DEFAULT_TRACK_HEIGHT,
    project::{ProjectState, clip_drag::ClipMove},
};
use tonique_engine::edit::ClipId;

impl Timeline {
    /// Clips dragged by their header: the core's [`ClipMove`] decides where
    /// they land and what landing does; this draws them there. With Ctrl,
    /// copies of the selection move.
    pub fn handle_dragged_clips(
        &mut self,
        ui: &mut Ui,
        layout: &RowLayout,
        grabbed_clip: Option<ClipId>,
        viewport: Rect,
        state: &mut ProjectState,
        view: &mut TimelineView,
    ) {
        let mouse_pos = ui.ctx().input(|i| i.pointer.hover_pos());
        if self.clip_drag.is_none()
            && let Some(clip) = grabbed_clip
            && let Some(mouse_pos) = mouse_pos
        {
            let grab = view.x_to_beats(mouse_pos.x, viewport.span());
            let duplicate = ui.input(|i| i.modifiers.ctrl);
            self.clip_drag = Some(ClipMove::start(state, clip, grab, duplicate));
        }

        if let Some(mut moving) = self.clip_drag.take()
            && let Some(mouse_pos) = mouse_pos
        {
            // Below the last row: on tracks a drop adds.
            let pointer_track = layout
                .track_at(mouse_pos.y)
                .map_or(state.track_count(), |(index, _)| index);
            let pointer = view.x_to_beats(mouse_pos.x, viewport.span());
            let alt = ui.input(|i| i.modifiers.alt);
            moving.place(state, view, pointer, pointer_track, alt);

            // Show what the clips snapped to.
            if let Some(beats) = moving.snapped_to {
                let x = view.beats_to_x(beats, viewport.span());
                ui.painter_at(viewport).vline(
                    x,
                    viewport.y_range(),
                    Stroke::new(1., ui.app_theme().text_muted),
                );
            }

            // Tracks a drop will add past the last one, with the colours
            // they'll get: the clips going there are shown on them.
            let added = moving.added_tracks(state.track_count());
            let new_colors = self.new_track_lanes(ui, state, layout, viewport, added);
            for moved in &moving.clips {
                let track_index = moved.track_index;
                let y = layout.track_top(track_index);
                let width = view.beats_to_width(moved.clip.length);
                let x = view.beats_to_x(moved.clip.position, viewport.span());
                let mut collapsed = false;
                let mut color = track_index
                    .checked_sub(state.track_count())
                    .and_then(|new| new_colors.get(new).copied())
                    .unwrap_or(ui.app_theme().text_muted);
                let mut height = DEFAULT_TRACK_HEIGHT;
                let mut silenced = false;
                if let Some(track) = state.track_from_index(track_index) {
                    collapsed = track.collapsed;
                    silenced = track.is_silenced();
                    color = track.color.to_egui();
                    height = track.height;
                }
                ClipView {
                    clip: &moved.clip,
                    rect: Rect::from_min_size(pos2(x, y), vec2(width, height)),
                    viewport,
                    timeline_view: view,
                    color,
                    selected: true,
                    zone: None,
                    collapsed,
                    silenced,
                    ghost: false,
                }
                .preview(ui, state);
            }

            if !ui.input(|i| i.pointer.primary_down()) {
                moving.land(state);
            } else if !moving.duplicate || ui.input(|i| i.modifiers.ctrl) {
                self.clip_drag = Some(moving);
                autoscroll(ui, view, layout, viewport, mouse_pos);
            }
        }
    }
}
