use crate::{
    config::keymap::Action,
    core::state::{MASTER_TRACK_ID, PlaybackState, ProjectState},
    ui::{
        arrangement::{
            row_layout::RowLayout,
            ruler::Ruler,
            timeline::Timeline,
            track_headers::{FOOTER_PADDING, TrackHeaders},
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
    Align2, Color32, FontId, Frame, Margin, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2,
};

/// Empty bars after the end of the arrangement.
const TIMELINE_SLACK_BARS: f64 = 16.;
const EMPTY_LOGO_SIZE: f32 = 80.;
/// The edit cursor's arrow on the ruler: width and height.
const EDIT_CURSOR_ARROW: Vec2 = vec2(9., 6.);
/// The master's output scope, in the footer.
const SCOPE_WIDTH: f32 = 120.;

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
        egui::CentralPanel::default()
            .frame(
                Frame::central_panel(ui.style())
                    .inner_margin(Margin::ZERO)
                    .fill(ui.app_theme().bg_base),
            )
            .show(ui, |ui| {
                self.ui(ui, state, workspace, commands);
            });
    }

    fn ui(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        workspace: &mut Workspace,
        commands: &mut Commands,
    ) {
        if workspace.main_view == MainView::Graph {
            self.graph.ui(ui, state);
            return;
        }
        // Rename the selected track, unless the effects panel took it.
        if !commands.take(|a| a == Action::Rename).is_empty()
            && let Some(track) = state.selected_track()
        {
            self.tracks.rename(track.id);
        }
        let available_rect = ui.available_rect_before_wrap();
        // Draw navigation bar on top
        self.ruler.ui(ui, state, self.tracks.width);

        let (area, _) = ui.allocate_exact_size(ui.available_size(), Sense::all());
        // The tracks scroll above a footer holding the master.
        let footer_height = (state.master_track().height + FOOTER_PADDING).min(area.height());
        let viewport = Rect::from_min_max(area.min, pos2(area.max.x, area.max.y - footer_height));
        let footer = Rect::from_min_max(pos2(area.min.x, viewport.max.y), area.max);
        master_lane_ui(ui, state, footer, footer.max.x - self.tracks.width);
        ui.set_clip_rect(viewport);

        let timeline_viewport = Rect::from_min_max(
            viewport.min,
            pos2(viewport.max.x - self.tracks.width, viewport.max.y),
        );
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
        // Draw tracks
        // With the tracks a drop on the timeline would add.
        let new_tracks = self.timeline.new_tracks();
        self.tracks
            .ui(ui, state, workspace, &layout, viewport, footer, new_tracks);
        if state.track_count() == 0 {
            empty_project_ui(ui, state, timeline_viewport);
        }

        // The timeline spans the content plus some room to add more, and
        // never shrinks under the part being looked at.
        let slack = TIMELINE_SLACK_BARS * state.grid.beats_per_bar() as f64;
        let visible_end = state
            .grid
            .width_to_beats(state.grid.offset.x + timeline_viewport.width());
        let end = (state.arrangement_end() + slack).0.max(visible_end);
        ui.set_width(state.grid.beats_to_width(end));
        // The footer isn't content to scroll to.
        let content_size = ui.min_size() - vec2(0., footer_height);

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
        let mut scrolled_x = false;
        if content_size.x > viewport.max.x {
            let track = Rect::from_min_max(
                pos2(viewport.left(), viewport.bottom() - SCROLLBAR_WIDTH),
                pos2(viewport.right() - self.tracks.width, viewport.bottom()),
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
                pos2(viewport.right() - SCROLLBAR_WIDTH, viewport.top()),
                pos2(viewport.right(), viewport.bottom()),
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

/// The footer's background, and the master's output scope at the end of
/// its timeline part (up to `timeline_right`), next to the master's header.
fn master_lane_ui(ui: &Ui, state: &ProjectState, footer: Rect, timeline_right: f32) {
    let theme = ui.app_theme();
    let painter = ui.painter_at(footer);
    painter.rect_filled(footer, 0., theme.bg_panel);
    painter.hline(
        footer.x_range(),
        footer.top() + 0.5,
        Stroke::new(1.0, theme.border),
    );
    let lane = Rect::from_min_max(footer.min, pos2(timeline_right, footer.max.y));

    let scope = Rect::from_min_max(
        pos2(
            lane.right() - SCOPE_WIDTH - 4.,
            lane.top() + FOOTER_PADDING + 2.,
        ),
        pos2(lane.right() - 4., lane.bottom() - 4.),
    );
    if scope.width() <= 0. || scope.height() <= 0. || lane.width() < 2. * SCOPE_WIDTH {
        return;
    }
    painter.rect_filled(scope, 2., theme.bg_deep);
    painter.text(
        scope.left_top() + vec2(4., 2.),
        Align2::LEFT_TOP,
        "OUT",
        FontId::proportional(font_size::SMALL),
        theme.text_disabled,
    );
    let samples = state
        .metrics
        .tracks
        .get(&MASTER_TRACK_ID)
        .filter(|m| m.samples.len() >= 2 && m.samples[0].len() > 3);
    let Some(m) = samples else {
        painter.hline(
            scope.x_range(),
            scope.center().y,
            Stroke::new(1.0, theme.separator),
        );
        return;
    };
    let len = m.samples[0].len() as f32;
    let points: Vec<_> = m.samples[0]
        .iter()
        .zip(m.samples[1].iter())
        .enumerate()
        .map(|(i, (l, r))| {
            pos2(
                scope.left() + i as f32 * scope.width() / len,
                scope.center().y - scope.height() * ((l + r) / 4.).clamp(-0.5, 0.5),
            )
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.0, theme.accent)));
    ui.interact(scope, ui.id().with("master-scope"), Sense::hover())
        .on_hover_text("Master output");
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
        FontId::proportional(font_size::HEADING),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_edit_cursor_points_down_at_the_timeline() {
        let [left, right, tip] = edit_cursor_arrow(50., 30.);
        assert_eq!(tip, pos2(50., 30.), "on the ruler's bottom");
        assert!(left.y < tip.y && left.y == right.y);
        assert_eq!((left.x + right.x) / 2., 50., "centred on the cursor");
    }
    use tonique_engine::engine::{Engine, EngineConfig};

    /// The track list and timeline draw nested and collapsed groups without
    /// trouble, headless.
    #[test]
    fn draws_groups_headless() {
        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        let tracks: Vec<_> = (0..5).map(|_| state.add_track()).collect();
        let inner = state.group(&tracks[..2]).unwrap();
        let outer = state.group(&[inner, tracks[2]]).unwrap();
        state.group(&[tracks[4]]).unwrap();
        state.track_view_mut(&outer).collapsed = false;
        state.track_view_mut(&inner).collapsed = true;

        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let input = || egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200., 800.))),
            ..Default::default()
        };
        let mut panel = CentralPanel::new();
        let mut shapes = 0;
        for _ in 0..2 {
            let mut output = ctx.run_ui(input(), |ui| {
                panel.show(
                    ui,
                    &mut state,
                    &mut Workspace::default(),
                    &mut Commands::default(),
                )
            });
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
        use crate::ui::arrangement::ruler::RULER_HEIGHT;
        use crate::{analysis::AudioInfo, ui::dnd::DragPayload};
        use egui::{Event, PointerButton, Pos2};

        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
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
        let mut frame = |state: &mut ProjectState, events: Vec<Event>| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 800.))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                panel.show(
                    ui,
                    state,
                    &mut Workspace::default(),
                    &mut Commands::default(),
                )
            });
            output.textures_delta.clear();
        };
        let collapsed = |state: &ProjectState| {
            state
                .rows()
                .iter()
                .find(|r| r.id == group)
                .unwrap()
                .collapsed
        };
        // The group's row is the first lane, just below the ruler.
        let over_group = Pos2::new(300., RULER_HEIGHT + 10.);
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

    /// Dragging a file below the last track shows the track a drop would
    /// add: a header with the name it'll get.
    #[test]
    fn dragging_past_the_last_track_shows_a_ghost_track() {
        use crate::{analysis::AudioInfo, ui::dnd::DragPayload};
        use egui::{Event, Pos2};

        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        state.add_track();
        state.add_track();
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let mut panel = CentralPanel::new();
        let mut texts = |state: &mut ProjectState, events: Vec<Event>| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 800.))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                panel.show(
                    ui,
                    state,
                    &mut Workspace::default(),
                    &mut Commands::default(),
                )
            });
            output.textures_delta.clear();
            output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        texts(&mut state, Vec::new()); // lay out once
        egui::DragAndDrop::set_payload(&ctx, DragPayload::File(AudioInfo::silent(1.)));
        let shown = texts(&mut state, vec![Event::PointerMoved(Pos2::new(300., 500.))]);
        assert!(
            shown.iter().any(|t| t == "3 Audio Track"),
            "the header: {shown:?}"
        );

        egui::DragAndDrop::clear_payload(&ctx);
        let shown = texts(&mut state, vec![Event::PointerMoved(Pos2::new(300., 500.))]);
        assert!(!shown.iter().any(|t| t == "3 Audio Track"), "gone after");
    }

    /// On the frame a drop adds the track, its header is shown where its
    /// ghost was, and the ghost is gone: no frame without it.
    #[test]
    fn dropping_past_the_last_track_shows_its_header_at_once() {
        use crate::{analysis::AudioInfo, ui::dnd::DragPayload};
        use egui::{Event, PointerButton, Pos2};

        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        state.add_track();
        state.add_track();
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        // The app's spacing, which the headers stack with.
        crate::ui::theme::Theme::default().install(&ctx);
        let mut panel = CentralPanel::new();
        // Where "3 Audio Track" is shown, each time.
        let mut names = |state: &mut ProjectState, events: Vec<Event>| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 800.))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                panel.show(
                    ui,
                    state,
                    &mut Workspace::default(),
                    &mut Commands::default(),
                )
            });
            output.textures_delta.clear();
            output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text() == "3 Audio Track" => Some(t.pos),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let below = Pos2::new(300., 500.);
        names(&mut state, Vec::new()); // lay out once
        egui::DragAndDrop::set_payload(&ctx, DragPayload::File(AudioInfo::silent(1.)));
        let ghost = names(&mut state, vec![Event::PointerMoved(below)]);
        assert_eq!(ghost.len(), 1, "the ghost");

        let release = Event::PointerButton {
            pos: below,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        };
        let dropped = names(&mut state, vec![release]);
        assert_eq!(state.track_count(), 3);
        assert_eq!(dropped, ghost, "the real header, in the ghost's place");
    }

    /// A rubber band starts where the button went down, between grid lines
    /// as much as on them.
    #[test]
    fn rubber_bands_start_where_pressed() {
        use crate::ui::arrangement::ruler::RULER_HEIGHT;
        use egui::{Event, PointerButton, Pos2};

        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        state.add_track();
        state.add_track();
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let mut panel = CentralPanel::new();
        let mut time = 0.;
        let mut frame = |state: &mut ProjectState, events: Vec<Event>| {
            time += 0.05;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 800.))),
                time: Some(time),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                panel.show(
                    ui,
                    state,
                    &mut Workspace::default(),
                    &mut Commands::default(),
                )
            });
            output.textures_delta.clear();
        };
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(&mut state, Vec::new()); // lay out once
        // Halfway between two beats, in the first lane.
        let ppb = state.grid.pixels_per_beat();
        let from = Pos2::new(2.5 * ppb, RULER_HEIGHT + 20.);
        frame(
            &mut state,
            vec![Event::PointerMoved(from), button(from, true)],
        );
        for step in 1..=5 {
            let at = from + vec2(step as f32 * 30., 40.);
            frame(&mut state, vec![Event::PointerMoved(at)]);
        }
        let bounds = state.selection_bounds().expect("a zone");
        assert!(
            (bounds.start_pos.0 - 2.5).abs() < 1e-3,
            "starts at {:?}",
            bounds.start_pos
        );
    }

    /// The rename shortcut types a name in the selected track's header;
    /// Enter keeps it, as an undo step.
    #[test]
    fn rename_shortcut_renames_the_selected_track() {
        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        state.add_track();
        let track = state.add_track();
        state.select_track(&track);

        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let mut panel = CentralPanel::new();
        let mut frame = |state: &mut ProjectState, events: Vec<egui::Event>, actions: &[Action]| {
            let mut commands = Commands::default();
            for action in actions {
                commands.push(*action);
            }
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200., 800.))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                panel.show(ui, state, &mut Workspace::default(), &mut commands)
            });
            output.textures_delta.clear();
            commands.take_all()
        };
        let key = |key, modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        frame(&mut state, vec![], &[]);
        assert!(
            frame(&mut state, vec![], &[Action::Rename]).is_empty(),
            "taken"
        );
        frame(&mut state, vec![], &[]);
        frame(
            &mut state,
            vec![
                key(egui::Key::A, egui::Modifiers::COMMAND),
                egui::Event::Text("Bass".into()),
                key(egui::Key::Enter, Default::default()),
            ],
            &[],
        );
        frame(&mut state, vec![], &[]);
        let name = |state: &ProjectState| state.selected_track().unwrap().name;
        assert_eq!(name(&state), "Bass");
        state.undo();
        assert_ne!(name(&state), "Bass");
    }
}
