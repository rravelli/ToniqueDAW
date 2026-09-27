use crate::{
    core::{clip::ClipCore, state::ToniqueProjectState, track::TRACK_CLOSED_HEIGHT},
    ui::{waveform::UIWaveform, widget::context_menu::ContextMenuButton},
};
use egui::{
    Align2, Color32, CursorIcon, FontFamily, FontId, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2,
    pos2, vec2,
};
use egui_phosphor::fill::TRASH;

const PADDING_TEXT: f32 = 4.;
const BORDER_WIDTH: f32 = 2.;
const HEADER_HEIGHT: f32 = 16.;
/// Width of the trim handles inside each edge of a clip.
const HANDLE_WIDTH: f32 = 7.;
const MIN_HANDLE_WIDTH: f32 = 2.;
const HANDLE_HOVER_COLOR: Color32 = Color32::from_rgba_premultiplied(140, 140, 140, 140);
#[derive(Clone, Copy)]
enum Edge {
    Start,
    End,
}

#[derive(Clone)]
pub struct UIClip {
    waveform: UIWaveform,
}

impl UIClip {
    pub fn new() -> Self {
        Self {
            waveform: UIWaveform::new(),
        }
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

        let stroke = if selected {
            Stroke::new(BORDER_WIDTH, Color32::WHITE)
        } else {
            Stroke::new(BORDER_WIDTH, color)
        };
        // Main rect
        painter.rect(
            Rect::from_min_size(pos, size),
            2.0,
            color.blend(Color32::from_white_alpha(20)),
            stroke,
            egui::StrokeKind::Inside,
        );
        // Grid
        if show_waveform {
            let rect = Rect::from_min_size(
                pos2(pos.x, pos.y + HEADER_HEIGHT - BORDER_WIDTH),
                vec2(size.x, size.y - HEADER_HEIGHT),
            );
            state.grid.render_clip_grid(&painter, viewport, rect, color);
        }
        // Header area
        let hitbox = Rect::from_min_size(
            Pos2::new(pos.x + BORDER_WIDTH, pos.y + BORDER_WIDTH),
            Vec2::new(
                size.x - 2. * BORDER_WIDTH,
                if show_waveform {
                    HEADER_HEIGHT
                } else {
                    TRACK_CLOSED_HEIGHT
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
        let handle_width = (size.x / 4.).clamp(MIN_HANDLE_WIDTH, HANDLE_WIDTH);
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
            Color32::BLACK,
        );

        painter.line(
            vec![hitbox.left_bottom(), hitbox.right_bottom()],
            Stroke::new(1.0, color.blend(Color32::from_black_alpha(50))),
        );
        // Waveform
        if show_waveform && let Ok(data) = clip.audio.data.read() {
            let mut shapes = Vec::new();
            let waveform_rect = Rect::from_min_max(
                Pos2::new(pos.x.max(viewport.left()), pos.y + HEADER_HEIGHT),
                Pos2::new((pos.x + size.x).min(viewport.right()), pos.y + size.y),
            );

            let start_ratio = (waveform_rect.left() - pos.x) / size.x
                * (clip.trim_end - clip.trim_start)
                + clip.trim_start;

            let end_ratio = clip.trim_end
                - (pos.x + size.x - waveform_rect.right()) / size.x
                    * (clip.trim_end - clip.trim_start);

            self.waveform.paint(
                &mut shapes,
                waveform_rect,
                data,
                start_ratio,
                end_ratio,
                clip.audio.num_samples.unwrap(),
                clip.audio.channels >= 2,
                Color32::BLACK,
            );
            painter.add(shapes);
        };
        // Draw an overlay when audio not ready
        if let Ok(ready) = clip.audio.ready.read()
            && !*ready
        {
            painter.rect_filled(sample_rect, 1.0, Color32::from_white_alpha(80));
        }

        response.context_menu(|ui| self.contex_menu(ui, clip, state));
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
            painter.rect_filled(rect, 1.0, HANDLE_HOVER_COLOR);
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
        }

        response
    }

    fn contex_menu(&self, ui: &mut Ui, clip: &ClipCore, state: &mut ToniqueProjectState) {
        ui.vertical(|ui| {
            if ui
                .add(ContextMenuButton::new(TRASH, "Delete").text_color(Color32::LIGHT_RED))
                .clicked()
            {
                state.delete_clips(&vec![clip.id.clone()]);
            };
        });
    }
}
