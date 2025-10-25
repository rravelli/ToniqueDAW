use crate::{
    core::{clip::ClipCore, state::ToniqueProjectState},
    ui::{
        grid::WorkspaceGrid, track::CLOSED_HEIGHT, waveform::UIWaveform,
        widget::context_menu::ContextMenuButton,
    },
};
use egui::{
    Align2, Color32, CursorIcon, FontFamily, FontId, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2,
};
use egui_phosphor::fill::TRASH;

const PADDING_TEXT: f32 = 4.;
const BORDER_WIDTH: f32 = 2.;
const HEADER_HEIGHT: f32 = 16.;
const MIN_HANDLE_WIDTH: f32 = 20.;
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
        grid: &WorkspaceGrid,
        selected: bool,
        clip: &ClipCore,
        state: &mut ToniqueProjectState,
        show_waveform: bool,
        color: Color32,
    ) -> Response {
        let sample_rect = Rect::from_min_max(viewport.clamp(pos), viewport.clamp(pos + size));
        // let response = ui.allocate_rect(sample_rect, Sense::all());
        let painter = ui.painter_at(sample_rect);
        let mut resized = false;
        let mut drag_stopped = false;
        let mut clip_copy = clip.clone();

        if size.x > MIN_HANDLE_WIDTH {
            let left_resize = self.left_resize_handle(
                ui,
                grid,
                Rect::from_min_size(Pos2::new(pos.x - 2.0, pos.y), Vec2::new(4., size.y)),
                state.bpm(),
                viewport,
                &mut clip_copy,
            );
            resized = resized || left_resize.dragged();
            drag_stopped = drag_stopped || left_resize.drag_stopped();
        }

        if size.x > MIN_HANDLE_WIDTH && pos.x + size.x + 2.0 < viewport.right() {
            let right_resize = self.right_resize_handle(
                ui,
                grid,
                Rect::from_min_size(
                    Pos2::new(pos.x + size.x - 2.0, pos.y),
                    Vec2::new(4., size.y),
                ),
                state.bpm(),
                viewport,
                &mut clip_copy,
            );
            resized = resized || right_resize.dragged();
            drag_stopped = drag_stopped || right_resize.drag_stopped();
        }

        let stroke = if selected {
            Stroke::new(BORDER_WIDTH, Color32::WHITE)
        } else {
            Stroke::new(BORDER_WIDTH, color)
        };
        painter.rect(
            Rect::from_min_size(pos, size),
            2.0,
            color
                .gamma_multiply_u8(230)
                .blend(Color32::from_white_alpha(30)),
            stroke,
            egui::StrokeKind::Inside,
        );
        let hitbox = Rect::from_min_size(
            Pos2::new(pos.x + BORDER_WIDTH, pos.y + BORDER_WIDTH),
            Vec2::new(
                size.x - 2. * BORDER_WIDTH,
                if show_waveform {
                    HEADER_HEIGHT
                } else {
                    CLOSED_HEIGHT
                } - 2. * BORDER_WIDTH,
            ),
        )
        .intersect(viewport);
        painter.rect(hitbox, 2.0, color, Stroke::NONE, egui::StrokeKind::Inside);
        let response = ui.interact(
            hitbox,
            format!("{}{}", clip.id, clip.position).into(),
            Sense::all(),
        );

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
            );
            painter.add(shapes);
        };

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

    fn left_resize_handle(
        &mut self,
        ui: &mut Ui,
        grid: &WorkspaceGrid,
        rect: Rect,
        bpm: f32,
        viewport: Rect,
        clip: &mut ClipCore,
    ) -> Response {
        let response = ui.allocate_rect(rect, Sense::drag());

        if response.dragged()
            && let Some(mouse_pos) = ui.input(|i| i.pointer.interact_pos())
        {
            clip.trim_start_at(
                grid.snap_at_grid_with_default(grid.x_to_beats(mouse_pos.x, viewport)),
                bpm,
            );
        }

        if response.hovered() {
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 1.0, Color32::WHITE);
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
        }

        response
    }

    fn right_resize_handle(
        &mut self,
        ui: &mut Ui,
        grid: &WorkspaceGrid,
        rect: Rect,
        bpm: f32,
        viewport: Rect,
        clip: &mut ClipCore,
    ) -> Response {
        let response = ui.allocate_rect(rect, Sense::drag());

        if response.dragged()
            && let Some(mouse_pos) = ui.input(|i| i.pointer.interact_pos())
        {
            clip.trim_end_at(
                grid.snap_at_grid_with_default(grid.x_to_beats(mouse_pos.x, viewport)),
                bpm,
            );
        }

        if response.hovered() {
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 1.0, Color32::WHITE);
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
