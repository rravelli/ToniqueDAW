mod master_lane;
#[cfg(test)]
mod tests;

use crate::ui::font::semibold;

use crate::{
    config::keymap::Action,
    core::state::{MASTER_TRACK_ID, PlaybackState, ProjectState},
    ui::{
        arrangement::{
            row_layout::RowLayout,
            ruler::Ruler,
            timeline::Timeline,
            track_headers::{FOOTER_PADDING, TrackHeaders, content_height, paint_drop_target},
        },
        commands::Commands,
        graph::GraphView,
        theme::{ThemeExt, font_size, with_alpha},
        widget::{
            logo::logo,
            scroll_bar::{Axis, SCROLLBAR_WIDTH, ScrollBar},
        },
        workspace::{MainView, Workspace},
    },
};
use egui::{
    Align2, Color32, FontId, Frame, Margin, Pos2, Rangef, Rect, Sense, Shape, Stroke, Ui, Vec2,
    pos2, vec2,
};

use master_lane::master_lane_ui;

/// Empty bars after the end of the arrangement.
const TIMELINE_SLACK_BARS: f64 = 16.;
const EMPTY_LOGO_SIZE: f32 = 80.;
/// The edit cursor's arrow on the ruler: width and height.
const EDIT_CURSOR_ARROW: Vec2 = vec2(9., 6.);
/// The master's output scope, in the footer.
const SCOPE_WIDTH: f32 = 180.;

pub struct CentralPanel {
    timeline: Timeline,
    ruler: Ruler,
    tracks: TrackHeaders,
    graph: GraphView,
    /// Following the playhead is paused after scrolling by hand during
    /// playback, until playback starts again.
    follow_paused: bool,
    was_playing: bool,
}

impl CentralPanel {
    /// Where the timeline was last drawn.
    #[cfg(test)]
    pub fn timeline_rect(&self) -> Rect {
        self.tracks.lanes()
    }

    pub fn new() -> Self {
        Self {
            timeline: Timeline::new(),
            ruler: Ruler::new(),
            tracks: TrackHeaders::new(),
            graph: GraphView::new(),
            follow_paused: false,
            was_playing: false,
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        workspace: &mut Workspace,
        commands: &mut Commands,
    ) {
        // The track list, as a panel of its own beside the timeline.
        let drop_target = if workspace.main_view == MainView::Timeline {
            self.tracks
                .show(ui, state, workspace, self.timeline.new_tracks())
        } else {
            None
        };
        egui::CentralPanel::default()
            .frame(
                Frame::central_panel(ui.style())
                    .inner_margin(Margin::ZERO)
                    .fill(ui.app_theme().bg_base),
            )
            .show(ui, |ui| {
                self.ui(ui, state, workspace, commands, drop_target);
            });
    }

    fn ui(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        workspace: &mut Workspace,
        commands: &mut Commands,
        drop_target: Option<Rangef>,
    ) {
        if workspace.main_view == MainView::Graph {
            self.graph.ui(ui, state);
            return;
        }
        // Rename the selected track, unless the effects panel took it.
        // The master keeps its name.
        if !commands.take(|a| a == Action::Rename).is_empty()
            && let Some(track) = state.selected_track()
            && track.id != MASTER_TRACK_ID
        {
            self.tracks.rename(track.id);
        }
        let available_rect = ui.available_rect_before_wrap();
        // Draw navigation bar on top
        self.ruler.ui(ui, state);

        let (area, _) = ui.allocate_exact_size(ui.available_size(), Sense::all());
        // The tracks scroll above a footer holding the master.
        let footer_height = (state.master_track().height + FOOTER_PADDING).min(area.height());
        let viewport = Rect::from_min_max(area.min, pos2(area.max.x, area.max.y - footer_height));
        let footer = Rect::from_min_max(pos2(area.min.x, viewport.max.y), area.max);
        master_lane_ui(ui, state, footer);
        ui.set_clip_rect(viewport);

        let timeline_viewport = viewport;
        let mut layout = RowLayout::new(state, viewport.top(), state.grid.offset.y);
        // Draw timeline
        self.timeline.ui(
            ui,
            state,
            workspace,
            commands,
            &mut layout,
            timeline_viewport,
        );
        self.tracks.set_lanes(timeline_viewport);
        // The lane of the track an effect would be dropped on, lit with
        // its header.
        // Over the footer too, for the master.
        if let Some(rows) = drop_target {
            let lanes = Rect::from_x_y_ranges(timeline_viewport.x_range(), area.y_range());
            let clip = ui.clip_rect();
            ui.set_clip_rect(lanes);
            paint_drop_target(ui, Rect::from_x_y_ranges(timeline_viewport.x_range(), rows));
            ui.set_clip_rect(clip);
        }
        if state.track_count() == 0 {
            empty_project_ui(ui, state, timeline_viewport);
        }

        // The timeline spans the content plus some room to add more, and
        // never shrinks under the part being looked at.
        let slack = TIMELINE_SLACK_BARS * state.grid.beats_per_bar();
        let visible_end = state
            .grid
            .width_to_beats(state.grid.offset.x + timeline_viewport.width());
        let end = (state.arrangement_end() + slack).0.max(visible_end);
        ui.set_width(state.grid.beats_to_width(end));
        // The track list is a panel of its own, so its rows don't size this
        // ui: scroll as far down as they go. The footer isn't content.
        let content_size = vec2(ui.min_size().x, viewport.top() + content_height(&layout));

        let mut scrolled_x = false;
        if ui.input(|i| i.smooth_scroll_delta != Vec2::ZERO && !i.modifiers.ctrl) {
            let delta = ui.input(|i| i.smooth_scroll_delta);
            // Not through a modal or menu above (unclipped: the track
            // list is beside this panel).
            let over = |rect| ui.ctx().rect_contains_pointer(ui.layer_id(), rect);
            if over(timeline_viewport) {
                scrolled_x |= delta.x != 0.;
                state.grid.offset.x -= delta.x;
                let max_x = (content_size.x - timeline_viewport.right()).max(0.);
                state.grid.offset.x = state.grid.offset.x.clamp(0., max_x);
            }
            if over(viewport) || over(self.tracks.rows_rect()) {
                state.grid.offset.y -= delta.y;
                let max_y = (content_size.y - viewport.bottom()).max(0.);
                state.grid.offset.y = state.grid.offset.y.clamp(0., max_y);
            }
        }
        scrolled_x |= self.scrollbars_ui(ui, viewport, content_size, &mut state.grid.offset);
        self.follow_playhead(ui, state, timeline_viewport, scrolled_x);
        // From the ruler's top, for the playhead's handle, to the footer.
        let cursors = Rect::from_min_max(
            available_rect.min,
            pos2(timeline_viewport.right(), timeline_viewport.bottom()),
        );
        self.cursors_ui(ui, state, cursors, timeline_viewport);
    }

    /// Keep the playhead in the middle of the view during playback. Near the
    /// start of the arrangement the view stays put and the playhead moves
    /// towards the middle.
    fn follow_playhead(
        &mut self,
        ui: &Ui,
        state: &mut ProjectState,
        viewport: Rect,
        scrolled_by_hand: bool,
    ) {
        let playing = state.playback_state() == PlaybackState::Playing;
        if playing && !self.was_playing {
            self.follow_paused = false;
        }
        self.was_playing = playing;
        if playing && scrolled_by_hand {
            self.follow_paused = true;
        }
        // Don't move the view under a drag.
        if !playing
            || !state.follow_playhead()
            || self.follow_paused
            || ui.input(|i| i.pointer.any_down())
        {
            return;
        }
        let x = state.grid.beats_to_width(state.playhead().0);
        state.grid.offset.x = (x - viewport.width() / 2.).max(0.);
    }

    /// The timeline's scrollbars, when the content overflows. Returns
    /// whether the horizontal one was dragged.
    fn scrollbars_ui(
        &self,
        ui: &mut Ui,
        viewport: Rect,
        content_size: Vec2,
        offset: &mut Vec2,
    ) -> bool {
        let lanes_right = viewport.right();
        let mut scrolled_x = false;
        if content_size.x > viewport.max.x {
            let track = Rect::from_min_max(
                pos2(viewport.left(), viewport.bottom() - SCROLLBAR_WIDTH),
                pos2(lanes_right - SCROLLBAR_WIDTH, viewport.bottom()),
            );
            scrolled_x = ui
                .add(ScrollBar::new(
                    "hscroll",
                    Axis::Horizontal,
                    track,
                    &mut offset.x,
                    content_size.x - viewport.right(),
                    viewport.width() / content_size.x,
                ))
                .dragged();
        }
        if content_size.y > viewport.max.y {
            let track = Rect::from_min_max(
                pos2(lanes_right - SCROLLBAR_WIDTH, viewport.top()),
                pos2(lanes_right, viewport.bottom()),
            );
            ui.add(ScrollBar::new(
                "vscroll",
                Axis::Vertical,
                track,
                &mut offset.y,
                content_size.y - viewport.bottom(),
                viewport.height() / (content_size.y - viewport.top()),
            ));
        }
        scrolled_x
    }

    /// Loop bounds (while looping), edit cursor (an arrow on the ruler,
    /// only while it differs from the playhead) and playhead with its
    /// draggable handle. `rect` spans the ruler and the timeline, `lanes`
    /// the timeline alone.
    fn cursors_ui(&self, ui: &mut Ui, state: &mut ProjectState, rect: Rect, lanes: Rect) {
        ui.set_clip_rect(rect);
        let theme = ui.app_theme();
        let painter = ui.painter();
        if state.looping() {
            let (start, end) = state.loop_range();
            for beats in [start, end] {
                let x = state.grid.beats_to_x(beats, rect);
                painter.vline(x, rect.y_range(), Stroke::new(1.0, theme.loop_region));
            }
        }
        if state.edit_cursor() != state.playhead() {
            // An arrow on the ruler, pointing down at the timeline.
            let x = state.grid.beats_to_x(state.edit_cursor(), rect);
            painter.add(Shape::convex_polygon(
                edit_cursor_arrow(x, lanes.top()).to_vec(),
                theme.edit_cursor,
                Stroke::NONE,
            ));
        }
        let playhead_x = state.grid.beats_to_x(state.playhead(), rect);
        let line_stroke = Stroke::new(2.0, with_alpha(theme.playhead, 160));

        // Draw vertical playhead line
        painter.line_segment(
            [
                pos2(playhead_x, rect.top()),
                pos2(playhead_x, rect.bottom()),
            ],
            line_stroke,
        );

        let handle_width = 8.0;
        let handle_height = 10.0;

        let points = [
            pos2(playhead_x, rect.top() + handle_height), // top (point)
            pos2(
                playhead_x - handle_width * 0.5,
                rect.top() + handle_height * 0.7,
            ),
            pos2(playhead_x - handle_width * 0.5, rect.top()),
            pos2(playhead_x + handle_width * 0.5, rect.top()),
            pos2(
                playhead_x + handle_width * 0.5,
                rect.top() + handle_height * 0.7,
            ),
        ];

        // Handle area for interaction
        let handle_rect = Rect::from_min_max(
            pos2(playhead_x - handle_width, rect.top()),
            pos2(playhead_x + handle_width, rect.top() + handle_height + 6.0),
        );

        let handle_response = ui.interact(
            handle_rect,
            ui.id().with("playhead_handle"),
            Sense::click_and_drag(),
        );

        // Highlight on hover
        let triangle_color = if handle_response.hovered() {
            theme.accent
        } else {
            theme.playhead
        };

        // Draw filled triangle
        painter.add(egui::Shape::convex_polygon(
            points.to_vec(),
            triangle_color,
            Stroke::NONE,
        ));

        // Dragging logic
        if handle_response.dragged()
            && let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos())
        {
            state.seek(state.grid.x_to_beats(mouse_pos.x, rect));
        }
    }
}

/// What a project without tracks shows: the logo, faded, and how to start.
fn empty_project_ui(ui: &Ui, state: &ProjectState, viewport: Rect) {
    let theme = ui.app_theme();
    let painter = ui.painter_at(viewport);
    let logo_rect = Rect::from_center_size(
        viewport.center() - vec2(0., EMPTY_LOGO_SIZE / 2.),
        Vec2::splat(EMPTY_LOGO_SIZE),
    );
    logo()
        .tint(Color32::WHITE.gamma_multiply(0.45))
        .paint_at(ui, logo_rect);
    let title = painter.text(
        logo_rect.center_bottom() + vec2(0., 12.),
        Align2::CENTER_TOP,
        "Tonique",
        semibold(font_size::HEADING),
        theme.text_muted,
    );
    let add_track = state
        .settings()
        .keymap
        .shortcut_text(ui.ctx(), Action::AddTrack);
    let hint = if add_track.is_empty() {
        "Drop audio files here to start".to_string()
    } else {
        format!("Drop audio files here, or press {add_track} to add a track")
    };
    painter.text(
        title.center_bottom() + vec2(0., 6.),
        Align2::CENTER_TOP,
        hint,
        FontId::proportional(font_size::LABEL),
        theme.text_muted,
    );
}

/// The edit cursor's arrow at `x`, its tip on the ruler's bottom (`bottom`).
fn edit_cursor_arrow(x: f32, bottom: f32) -> [Pos2; 3] {
    let half = EDIT_CURSOR_ARROW.x / 2.;
    let top = bottom - EDIT_CURSOR_ARROW.y;
    [pos2(x - half, top), pos2(x + half, top), pos2(x, bottom)]
}
