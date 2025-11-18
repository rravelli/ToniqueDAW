use crate::{
    core::state::ToniqueProjectState,
    ui::{
        theme::PRIMARY_COLOR,
        view::{
            navigation_bar::{NAVIGATION_BAR_HEIGHT, UINavigationBar},
            timeline::UITimeline,
            tracks::UITracks,
        },
    },
};
use egui::{Color32, Context, Frame, Margin, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

pub const SCROLLBAR_WIDTH: f32 = 5.;
pub const PLAYHEAD_COLOR: Color32 = Color32::WHITE;

pub struct UICentralPanel {
    timeline: UITimeline,
    navigation_bar: UINavigationBar,
    tracks: UITracks,
}

impl UICentralPanel {
    pub fn new() -> Self {
        Self {
            timeline: UITimeline::new(),
            navigation_bar: UINavigationBar::new(),
            tracks: UITracks::new(),
        }
    }

    pub fn show(&mut self, ctx: &Context, state: &mut ToniqueProjectState) {
        egui::CentralPanel::default()
            .frame(
                Frame::central_panel(&ctx.style())
                    .inner_margin(Margin::ZERO)
                    .fill(Color32::from_gray(55)),
            )
            .show(ctx, |ui| {
                self.ui(ui, state);
            });
    }

    fn ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
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

        // Set timeline width
        ui.set_width(1000. * state.grid.pixels_per_beat());
        let content_size = ui.min_size();

        if let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos())
            && ui.input(|i| i.smooth_scroll_delta != Vec2::ZERO && !i.modifiers.alt)
        {
            let delta = ui.input(|i| i.smooth_scroll_delta);
            if timeline_viewport.contains(mouse_pos) {
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
        let rect = Rect::from_min_size(
            available_rect.min,
            vec2(
                available_rect.width() - self.tracks.width,
                available_rect.height(),
            ),
        );
        self.draw_scrollbars(ui, viewport, content_size, &mut state.grid.offset);
        ui.set_clip_rect(rect);
        self.draw_loop_markers(ui, state, rect);
        self.draw_playhead_handle(ui, state, rect);
    }

    /// Draw horizontal and vertical scrollbars.
    fn draw_scrollbars(&self, ui: &mut Ui, viewport: Rect, content_size: Vec2, offset: &mut Vec2) {
        let style = ui.style();
        let painter = ui.painter();
        let handle_color = Color32::WHITE;

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
            painter.rect_filled(track_rect, 2.0, style.visuals.extreme_bg_color);
            painter.rect_filled(thumb_rect, 4.0, handle_color);

            if resp.dragged() {
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
            painter.rect_filled(track_rect, 2.0, style.visuals.extreme_bg_color);
            painter.rect_filled(thumb_rect, 4.0, handle_color);

            if resp.dragged() {
                let drag_y = resp.drag_delta().y;

                let ratio = max_scroll / (track_rect.height() - thumb_height);
                offset.y = (offset.y + drag_y * ratio).clamp(0., max_scroll);
            }
        }
    }

    fn draw_loop_markers(&self, ui: &mut Ui, state: &mut ToniqueProjectState, rect: Rect) {
        let start_x = state.grid.beats_to_x(state.loop_state().start, rect);
        let end_x = state.grid.beats_to_x(state.loop_state().end, rect);
        let triangle_height = 8.;
        let color = PRIMARY_COLOR;

        if state.loop_state().enabled {
            ui.painter().rect_filled(
                Rect::from_min_max(pos2(start_x, rect.top()), pos2(end_x, rect.top() + 12.)),
                2.0,
                PRIMARY_COLOR.gamma_multiply(0.4),
            );
        }

        ui.painter().line_segment(
            [pos2(start_x, rect.top()), pos2(start_x, rect.bottom())],
            Stroke::new(2., color),
        );

        let points = [
            pos2(start_x, rect.top() + triangle_height), // top (point)
            pos2(start_x + triangle_height, rect.top()),
            pos2(start_x, rect.top()),
        ];

        ui.painter().add(egui::Shape::convex_polygon(
            points.to_vec(),
            color,
            Stroke::new(0., Color32::from_black_alpha(120)),
        ));

        let points = [
            pos2(end_x, rect.top() + triangle_height), // top (point)
            pos2(end_x - triangle_height, rect.top()),
            pos2(end_x, rect.top()),
        ];

        ui.painter().add(egui::Shape::convex_polygon(
            points.to_vec(),
            color,
            Stroke::new(0., Color32::from_black_alpha(120)),
        ));
        ui.painter().line_segment(
            [pos2(end_x, rect.top()), pos2(end_x, rect.bottom())],
            Stroke::new(2., color),
        );

        let handle_rect = Rect::from_min_max(
            pos2(start_x - 1., rect.top()),
            pos2(
                start_x + triangle_height,
                rect.top() + NAVIGATION_BAR_HEIGHT,
            ),
        );

        let start_handle_response = ui.interact(
            handle_rect,
            ui.id().with("loop-start-handle"),
            Sense::click_and_drag(),
        );

        if start_handle_response.dragged()
            && let Some(pos) = start_handle_response.interact_pointer_pos()
        {
            let snapped_position = state.grid.snap_at_grid(state.grid.x_to_beats(pos.x, rect));
            state.set_loop_start(snapped_position);
        }

        let handle_rect = Rect::from_min_max(
            pos2(end_x - triangle_height / 2., rect.top()),
            pos2(end_x + 1., rect.top() + NAVIGATION_BAR_HEIGHT),
        );

        let handle_response = ui.interact(
            handle_rect,
            ui.id().with("loop-end-handle"),
            Sense::click_and_drag(),
        );

        if handle_response.dragged()
            && let Some(pos) = handle_response.interact_pointer_pos()
        {
            let snapped_position = state.grid.snap_at_grid(state.grid.x_to_beats(pos.x, rect));
            state.set_loop_end(snapped_position);
        }
    }

    fn draw_playhead_handle(&self, ui: &mut Ui, state: &mut ToniqueProjectState, rect: Rect) {
        let painter = ui.painter();
        let playhead_x = state.grid.beats_to_x(state.playback_position(), rect);
        let line_stroke = Stroke::new(2.0, PLAYHEAD_COLOR.gamma_multiply_u8(160));

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
            PLAYHEAD_COLOR.blend(Color32::from_white_alpha(50))
        } else {
            PLAYHEAD_COLOR
        };

        // Draw filled triangle
        painter.add(egui::Shape::convex_polygon(
            points.to_vec(),
            triangle_color,
            Stroke::new(0., Color32::from_black_alpha(120)),
        ));

        // Dragging logic
        if handle_response.dragged() {
            if let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos()) {
                let new_beat = state.grid.x_to_beats(mouse_pos.x, rect);
                state.set_playback_position(new_beat);
            }
        }
    }
}
