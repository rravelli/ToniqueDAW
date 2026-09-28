use crate::{
    core::state::{CentralView, PlaybackState, ToniqueProjectState},
    ui::{
        theme::{ThemeExt, with_alpha},
        view::{
            graph::GraphView, navigation_bar::NavigationBar, timeline::Timeline,
            tracks::TrackHeaders,
        },
    },
};
use egui::{Frame, Margin, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

pub const SCROLLBAR_WIDTH: f32 = 5.;
/// Empty bars after the end of the arrangement.
const TIMELINE_SLACK_BARS: f32 = 16.;

pub struct CentralPanel {
    timeline: Timeline,
    navigation_bar: NavigationBar,
    tracks: TrackHeaders,
    graph: GraphView,
    /// Following the playhead is paused after scrolling by hand during
    /// playback, until playback starts again.
    follow_paused: bool,
    was_playing: bool,
}

impl CentralPanel {
    pub fn new() -> Self {
        Self {
            timeline: Timeline::new(),
            navigation_bar: NavigationBar::new(),
            tracks: TrackHeaders::new(),
            graph: GraphView::new(),
            follow_paused: false,
            was_playing: false,
        }
    }

    pub fn show(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        egui::CentralPanel::default()
            .frame(
                Frame::central_panel(ui.style())
                    .inner_margin(Margin::ZERO)
                    .fill(ui.app_theme().bg_base),
            )
            .show(ui, |ui| {
                self.ui(ui, state);
            });
    }

    fn ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        if state.central_view == CentralView::Graph {
            self.graph.ui(ui, state);
            return;
        }
        let available_rect = ui.available_rect_before_wrap();
        // Draw navigation bar on top
        self.navigation_bar.ui(ui, state, self.tracks.width);

        let (viewport, _) = ui.allocate_exact_size(ui.available_size(), Sense::all());
        ui.set_clip_rect(viewport);

        let timeline_viewport = Rect::from_min_max(
            viewport.min,
            pos2(viewport.max.x - self.tracks.width, viewport.max.y),
        );
        // Draw timeline
        self.timeline
            .ui(ui, state, timeline_viewport, state.grid.offset);
        // Draw tracks
        self.tracks.ui(ui, state, viewport);

        // The timeline spans the content plus some room to add more, and
        // never shrinks under the part being looked at.
        let ppb = state.grid.pixels_per_beat();
        let slack = TIMELINE_SLACK_BARS * state.grid.beats_per_bar() as f32;
        let visible_end = (state.grid.offset.x + timeline_viewport.width()) / ppb;
        ui.set_width((state.arrangement_end() + slack).max(visible_end) * ppb);
        let content_size = ui.min_size();

        let mut scrolled_x = false;
        if let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos())
            && ui.input(|i| i.smooth_scroll_delta != Vec2::ZERO && !i.modifiers.ctrl)
        {
            let delta = ui.input(|i| i.smooth_scroll_delta);
            if timeline_viewport.contains(mouse_pos) {
                scrolled_x |= delta.x != 0.;
                state.grid.offset.x -= delta.x;
                let max_x = (content_size.x - timeline_viewport.right()).max(0.);
                state.grid.offset.x = state.grid.offset.x.clamp(0., max_x);
            }
            if viewport.contains(mouse_pos) {
                state.grid.offset.y -= delta.y;
                let max_y = (content_size.y - viewport.bottom()).max(0.);
                state.grid.offset.y = state.grid.offset.y.clamp(0., max_y);
            }
        }
        scrolled_x |= self.scrollbars_ui(ui, viewport, content_size, &mut state.grid.offset);
        self.follow_playhead(ui, state, timeline_viewport, scrolled_x);
        self.cursors_ui(
            ui,
            state,
            Rect::from_min_size(
                available_rect.min,
                vec2(
                    available_rect.width() - self.tracks.width,
                    available_rect.height(),
                ),
            ),
        );
    }

    /// Keep the playhead in the middle of the view during playback. Near the
    /// start of the arrangement the view stays put and the playhead moves
    /// towards the middle.
    fn follow_playhead(
        &mut self,
        ui: &Ui,
        state: &mut ToniqueProjectState,
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
        let x = state.playhead() * state.grid.pixels_per_beat();
        state.grid.offset.x = (x - viewport.width() / 2.).max(0.);
    }

    /// Draw horizontal and vertical scrollbars. Returns whether the
    /// horizontal one was dragged.
    fn scrollbars_ui(
        &self,
        ui: &mut Ui,
        viewport: Rect,
        content_size: Vec2,
        offset: &mut Vec2,
    ) -> bool {
        let mut scrolled_x = false;
        let theme = ui.app_theme();
        let painter = ui.painter();
        let handle_color = theme.text_disabled;

        // === HORIZONTAL SCROLLBAR ===
        if content_size.x > viewport.max.x {
            let track_rect = Rect::from_min_max(
                pos2(viewport.left(), viewport.bottom() - SCROLLBAR_WIDTH),
                pos2(viewport.right() - self.tracks.width, viewport.bottom()),
            );
            let max_scroll = content_size.x - viewport.right();
            // Thumb size and position
            let visible_ratio_x = viewport.width() / content_size.x;
            let thumb_width = (visible_ratio_x * track_rect.width()).max(16.0);
            let thumb_x =
                track_rect.left() + (offset.x / max_scroll) * (track_rect.width() - thumb_width);

            let thumb_rect = Rect::from_min_max(
                pos2(thumb_x, track_rect.top()),
                pos2(thumb_x + thumb_width, track_rect.bottom()),
            );

            let resp = ui.interact(thumb_rect, ui.id().with("hscroll"), Sense::click_and_drag());
            painter.rect_filled(track_rect, 2.0, theme.bg_deep);
            painter.rect_filled(thumb_rect, 4.0, handle_color);

            if resp.dragged() {
                scrolled_x = true;
                let drag_x = resp.drag_delta().x;
                let ratio = max_scroll / (track_rect.width() - thumb_width);
                offset.x = (offset.x + drag_x * ratio).clamp(0., max_scroll);
            }
        }

        // === VERTICAL SCROLLBAR ===
        if content_size.y > viewport.max.y {
            let track_rect = Rect::from_min_max(
                pos2(viewport.right() - SCROLLBAR_WIDTH, viewport.top()),
                pos2(viewport.right(), viewport.bottom()),
            );
            let max_scroll = content_size.y - viewport.bottom();
            let visible_ratio_y = viewport.height() / (content_size.y - viewport.top());
            let thumb_height = (visible_ratio_y * track_rect.height()).max(16.0);
            let thumb_y = track_rect.top()
                + (offset.y / (content_size.y - viewport.bottom()))
                    * (track_rect.height() - thumb_height);

            let thumb_rect = Rect::from_min_max(
                pos2(track_rect.left(), thumb_y),
                pos2(track_rect.right(), thumb_y + thumb_height),
            );

            let resp = ui.interact(thumb_rect, ui.id().with("vscroll"), Sense::click_and_drag());
            painter.rect_filled(track_rect, 2.0, theme.bg_deep);
            painter.rect_filled(thumb_rect, 4.0, handle_color);

            if resp.dragged() {
                let drag_y = resp.drag_delta().y;

                let ratio = max_scroll / (track_rect.height() - thumb_height);
                offset.y = (offset.y + drag_y * ratio).clamp(0., max_scroll);
            }
        }
        scrolled_x
    }

    /// Loop bounds (while looping), edit cursor (only while it differs from
    /// the playhead) and playhead
    /// with its draggable handle.
    fn cursors_ui(&self, ui: &mut Ui, state: &mut ToniqueProjectState, rect: Rect) {
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
            let x = state.grid.beats_to_x(state.edit_cursor(), rect);
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1.0, theme.edit_cursor),
            );
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

#[cfg(test)]
mod tests {
    use super::*;
    use tonique_engine::engine::{Engine, EngineConfig};

    /// The track list and timeline draw nested and collapsed groups without
    /// trouble, headless.
    #[test]
    fn draws_groups_headless() {
        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ToniqueProjectState::new(engine);
        let tracks: Vec<_> = (0..5).map(|_| state.add_track()).collect();
        let inner = state.group(&tracks[..2]).unwrap();
        let outer = state.group(&[inner, tracks[2]]).unwrap();
        state.group(&[tracks[4]]).unwrap();
        state.track_mut(&outer).collapsed = false;
        state.track_mut(&inner).collapsed = true;

        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let input = || egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200., 800.))),
            ..Default::default()
        };
        let mut panel = CentralPanel::new();
        let mut shapes = 0;
        for _ in 0..2 {
            let mut output = ctx.run_ui(input(), |ui| panel.show(ui, &mut state));
            output.textures_delta.clear();
            shapes = output.shapes.len();
        }
        assert!(shapes > 0);
        assert_eq!(
            state.rows().len(),
            6,
            "outer, inner (collapsed), t3, t4, group, t5"
        );
    }

    /// Dragging a file over a collapsed group expands it; leaving collapses
    /// it again; dropping inside leaves it expanded.
    #[test]
    fn collapsed_groups_expand_while_dragging_over_them() {
        use crate::ui::view::navigation_bar::NAVIGATION_BAR_HEIGHT;
        use crate::{analysis::AudioInfo, ui::panels::left_panel::DragPayload};
        use egui::{Event, PointerButton, Pos2};

        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ToniqueProjectState::new(engine);
        let tracks: Vec<_> = (0..3).map(|_| state.add_track()).collect();
        let group = state.group(&tracks[..2]).unwrap();
        state.set_collapsed(&group, true);

        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let mut panel = CentralPanel::new();
        let audio = AudioInfo {
            name: "a.wav".into(),
            duration: Some(std::time::Duration::from_secs(1)),
            data: Default::default(),
            sample_rate: 48000,
            channels: 2,
            bit_depth: None,
            num_samples: None,
            path: Default::default(),
        };
        let mut frame = |state: &mut ToniqueProjectState, events: Vec<Event>| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 800.))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| panel.show(ui, state));
            output.textures_delta.clear();
        };
        let collapsed = |state: &ToniqueProjectState| {
            state
                .rows()
                .iter()
                .find(|r| r.id == group)
                .unwrap()
                .collapsed
        };
        // The group's row is the first lane, just below the ruler.
        let over_group = Pos2::new(300., NAVIGATION_BAR_HEIGHT + 10.);
        let far_below = Pos2::new(300., 600.);

        frame(&mut state, Vec::new()); // lay out once
        egui::DragAndDrop::set_payload(&ctx, DragPayload::File(audio.clone()));
        frame(&mut state, vec![Event::PointerMoved(over_group)]);
        assert!(!collapsed(&state), "expands while dragged over");

        frame(&mut state, vec![Event::PointerMoved(far_below)]);
        assert!(collapsed(&state), "collapses again once left");

        frame(&mut state, vec![Event::PointerMoved(over_group)]);
        assert!(!collapsed(&state));
        // Drop inside.
        let release = Event::PointerButton {
            pos: over_group,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        };
        frame(&mut state, vec![release]);
        frame(&mut state, vec![Event::PointerMoved(over_group)]);
        frame(&mut state, vec![Event::PointerMoved(far_below)]);
        assert!(!collapsed(&state), "stays expanded after a drop inside");
    }
}
