use crate::{
    core::{clip::ClipCore, state::ToniqueProjectState, track::TRACK_COLLAPSED_HEIGHT},
    ui::{
        theme::{ThemeExt, with_alpha},
        waveform::paint_waveform,
        widget::context_menu::ContextMenuButton,
    },
};
use egui::{
    Align2, Color32, CursorIcon, FontFamily, FontId, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2,
    pos2, vec2,
};
use egui_phosphor::fill::TRASH;
use std::time::Duration;

const PADDING_TEXT: f32 = 4.;
const BORDER_WIDTH: f32 = 2.;
const HEADER_HEIGHT: f32 = 16.;
/// Width of the trim handles inside each edge of a clip.
const TRIM_HANDLE_WIDTH: f32 = 7.;
const MIN_TRIM_HANDLE_WIDTH: f32 = 2.;
#[derive(Clone, Copy)]
enum Edge {
    Start,
    End,
}

const LOADING_REPAINT: Duration = Duration::from_millis(100);
#[derive(Clone)]
pub struct ClipView {}

impl ClipView {
    pub fn new() -> Self {
        Self {}
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        pos: Pos2,
        size: Vec2,
        viewport: Rect,
        selected: bool,
        clip: &ClipCore,
        state: &mut ToniqueProjectState,
        show_waveform: bool,
        color: Color32,
    ) -> Response {
        let sample_rect = Rect::from_min_max(viewport.clamp(pos), viewport.clamp(pos + size));
        // let response = ui.allocate_rect(sample_rect, Sense::all());
        let painter = ui.painter_at(sample_rect);
        let mut clip_copy = clip.clone();
        let theme = ui.app_theme();
        // Selected clips light up and get a neutral outline, rather than a
        // UI colour that could clash with the clip's own.
        let fill = if selected {
            color.lerp_to_gamma(theme.clip_selected, 0.2)
        } else {
            color
        };
        // Name and waveform are drawn on the clip's colour.
        let ink = theme.text_on(fill);

        let stroke = if selected {
            Stroke::new(BORDER_WIDTH, theme.clip_selected)
        } else {
            Stroke::new(BORDER_WIDTH, fill)
        };
        // Main rect
        painter.rect(
            Rect::from_min_size(pos, size),
            2.0,
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );
        // Grid
        if show_waveform {
            let rect = Rect::from_min_size(
                pos2(pos.x, pos.y + HEADER_HEIGHT - BORDER_WIDTH),
                vec2(size.x, size.y - HEADER_HEIGHT),
            );
            state
                .grid
                .paint_clip_grid(&painter, viewport, rect, with_alpha(ink, 25));
        }
        // Header area
        let hitbox = Rect::from_min_size(
            Pos2::new(pos.x + BORDER_WIDTH, pos.y + BORDER_WIDTH),
            Vec2::new(
                size.x - 2. * BORDER_WIDTH,
                if show_waveform {
                    HEADER_HEIGHT
                } else {
                    TRACK_COLLAPSED_HEIGHT
                } - 2. * BORDER_WIDTH,
            ),
        )
        .intersect(viewport);
        painter.rect(hitbox, 2.0, color, Stroke::NONE, egui::StrokeKind::Inside);

        let response = ui.interact(
            hitbox,
            format!("{:?}{}", clip.id, clip.position).into(),
            Sense::all(),
        );
        // Trim handles just inside each edge, registered after the header so
        // they win where they overlap it. Narrow clips get narrower handles,
        // leaving the middle to grab the clip.
        let handle_width = (size.x / 4.).clamp(MIN_TRIM_HANDLE_WIDTH, TRIM_HANDLE_WIDTH);
        let left_resize = self.trim_handle(
            ui,
            state,
            Rect::from_min_size(pos, vec2(handle_width, size.y)),
            viewport,
            &mut clip_copy,
            Edge::Start,
        );
        let right_resize = self.trim_handle(
            ui,
            state,
            Rect::from_min_size(
                pos2(pos.x + size.x - handle_width, pos.y),
                vec2(handle_width, size.y),
            ),
            viewport,
            &mut clip_copy,
            Edge::End,
        );
        let resized = left_resize.dragged() || right_resize.dragged();
        let drag_stopped = left_resize.drag_stopped() || right_resize.drag_stopped();
        painter.text(
            Pos2::new(pos.x + PADDING_TEXT, pos.y + 2.),
            Align2::LEFT_TOP,
            format!("{}", clip.audio.name.clone()),
            FontId::new(10., FontFamily::Monospace),
            ink,
        );

        painter.line(
            vec![hitbox.left_bottom(), hitbox.right_bottom()],
            Stroke::new(1.0, with_alpha(ink, 60)),
        );
        // Waveform
        if show_waveform && let Some(frames) = clip.audio.total_frames() {
            let waveform_rect = Rect::from_min_max(
                pos2(pos.x, pos.y + HEADER_HEIGHT),
                pos2(pos.x + size.x, pos.y + size.y),
            );
            // Lay out the whole untrimmed file and only show the clip's part,
            // so trimming doesn't shift which samples fall in each column.
            let source_width = state
                .grid
                .duration_to_width(clip.audio.duration.unwrap_or_default(), state.bpm());
            let source_rect = Rect::from_min_size(
                pos2(
                    waveform_rect.left() - clip.trim_start * source_width,
                    waveform_rect.top(),
                ),
                vec2(source_width, waveform_rect.height()),
            );
            paint_waveform(
                &painter,
                source_rect,
                waveform_rect.intersect(viewport),
                &clip.audio.data,
                0.0..frames,
                clip.audio.channels >= 2,
                ink,
            );
        }
        // Draw an overlay when audio not ready
        if !clip.audio.data.is_ready() {
            painter.rect_filled(sample_rect, 1.0, theme.shadow);
            // Show the waveform growing while the file decodes.
            ui.ctx().request_repaint_after(LOADING_REPAINT);
        }

        response.context_menu(|ui| self.context_menu(ui, clip, state));
        // Update cursor icon
        if response.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        };

        if resized {
            state.resize_clip(
                &clip_copy.id,
                clip_copy.trim_start,
                clip_copy.trim_end,
                clip_copy.position,
            );
        }
        if drag_stopped {
            state.commit_resize_clip(
                &clip_copy.id,
                clip_copy.trim_start,
                clip_copy.trim_end,
                clip_copy.position,
            );
        }

        response
    }

    /// A trim handle: dragging it moves that edge of `clip`, snapping to
    /// the grid, other clips' edges, the loop and the edit cursor (Alt
    /// disables snapping).
    fn trim_handle(
        &mut self,
        ui: &mut Ui,
        state: &ToniqueProjectState,
        rect: Rect,
        viewport: Rect,
        clip: &mut ClipCore,
        edge: Edge,
    ) -> Response {
        let response = ui.allocate_rect(rect, Sense::drag());

        if response.dragged()
            && let Some(mouse_pos) = ui.input(|i| i.pointer.interact_pos())
        {
            let beats = state.grid.x_to_beats(mouse_pos.x, viewport);
            let beats = if ui.input(|i| i.modifiers.alt) {
                beats
            } else {
                let targets = state.snap_targets(&[clip.id]);
                state
                    .grid
                    .snap_to_targets(beats, &targets)
                    .map_or(beats, |(snapped, _)| snapped)
            };
            match edge {
                Edge::Start => clip.trim_start_at(beats, state.bpm()),
                Edge::End => clip.trim_end_at(beats, state.bpm()),
            }
        }

        if response.hovered() {
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 1.0, ui.app_theme().hover_overlay);
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
        }

        response
    }

    fn context_menu(&self, ui: &mut Ui, clip: &ClipCore, state: &mut ToniqueProjectState) {
        ui.vertical(|ui| {
            if ui
                .add(ContextMenuButton::new(TRASH, "Delete").text_color(ui.app_theme().danger))
                .clicked()
            {
                state.delete_clips(&vec![clip.id.clone()]);
            };
        });
    }
}
