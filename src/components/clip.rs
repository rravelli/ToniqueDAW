use std::time::Duration;

use egui::{
    Align2, Color32, FontFamily, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2,
};
use rtrb::Producer;

use crate::{
    analysis::AudioInfo,
    components::{grid::WorkspaceGrid, waveform::UIWaveform},
    message::GuiToPlayerMsg,
};

const MIN_TRIM_RATIO: f32 = 1e-2;
const PADDING_TEXT: f32 = 4.;
const BORDER_WIDTH: f32 = 1.;

#[derive(Clone)]
pub struct UIClip {
    id: String,
    pub audio: AudioInfo,

    // Position in beat
    pub position: f32,

    pub trim_start: f32,
    pub trim_end: f32,

    waveform: UIWaveform,
}

impl UIClip {
    pub fn new(audio: AudioInfo, position: f32) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            audio,
            position,
            trim_start: 0.,
            trim_end: 1.,
            waveform: UIWaveform::new(),
        }
    }

    pub fn id(&self) -> String {
        self.id.clone()
    }

    pub fn end(&self, bpm: f32) -> f32 {
        self.position
            + self.audio.duration.unwrap().as_secs_f32() / 60.
                * bpm
                * (self.trim_end - self.trim_start)
    }

    pub fn duration(&self) -> Option<Duration> {
        if let Some(duration) = self.audio.duration {
            Some(Duration::from_secs_f32(
                duration.as_secs_f32() * (self.trim_end - self.trim_start),
            ))
        } else {
            None
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        shapes: &mut Vec<Shape>,
        pos: Pos2,
        size: Vec2,
        viewport: Rect,
        grid: &WorkspaceGrid,
        bpm: f32,
        track_id: String,
        selected: bool,
        tx: &mut Producer<GuiToPlayerMsg>,
        show_waveform: bool,
        color: Color32,
    ) -> Response {
        let sample_rect = Rect::from_min_max(viewport.clamp(pos), viewport.clamp(pos + size));
        let response = ui.allocate_rect(sample_rect, Sense::all());
        let painter = ui.painter_at(sample_rect);
        let mut resized = false;

        let left_resize = self.left_resize_handle(
            ui,
            grid,
            Rect::from_min_size(Pos2::new(pos.x - 2.0, pos.y), Vec2::new(4., size.y)),
            bpm,
        );

        resized = resized || left_resize.drag_stopped();

        if pos.x + size.x + 2.0 < viewport.right() {
            let right_resize = self.right_resize_handle(
                ui,
                grid,
                Rect::from_min_size(
                    Pos2::new(pos.x + size.x - 2.0, pos.y),
                    Vec2::new(4., size.y),
                ),
                bpm,
            );
            resized = resized || right_resize.drag_stopped();
        }

        let stroke = if selected {
            Stroke::new(BORDER_WIDTH, Color32::WHITE)
        } else {
            Stroke::new(BORDER_WIDTH, color)
        };

        painter.rect(
            Rect::from_min_size(pos, size),
            2.0,
            color.gamma_multiply(0.8),
            stroke,
            egui::StrokeKind::Inside,
        );
        painter.rect(
            Rect::from_min_size(
                Pos2::new(pos.x + BORDER_WIDTH, pos.y + BORDER_WIDTH),
                Vec2::new(size.x - 2. * BORDER_WIDTH, 12.0 - BORDER_WIDTH),
            ),
            2.0,
            color,
            Stroke::NONE,
            egui::StrokeKind::Inside,
        );

        painter.text(
            Pos2::new(pos.x + PADDING_TEXT, pos.y + 1.),
            Align2::LEFT_TOP,
            format!("{}", self.audio.name.clone()),
            FontId::new(10., FontFamily::Monospace),
            Color32::BLACK,
        );

        if show_waveform && let Ok(data) = self.audio.data.lock() {
            let waveform_rect = Rect::from_min_max(
                Pos2::new(pos.x.max(viewport.left()), pos.y + 12.),
                Pos2::new((pos.x + size.x).min(viewport.right()), pos.y + size.y),
            );

            let start_ratio = (waveform_rect.left() - pos.x) / size.x
                * (self.trim_end - self.trim_start)
                + self.trim_start;

            let end_ratio = self.trim_end
                - (pos.x + size.x - waveform_rect.right()) / size.x
                    * (self.trim_end - self.trim_start);

            self.waveform.paint(
                shapes,
                waveform_rect,
                data,
                start_ratio,
                end_ratio,
                self.audio.num_samples.unwrap(),
            );
        };

        if let Ok(ready) = self.audio.ready.lock()
            && !*ready
        {
            painter.rect_filled(sample_rect, 1.0, Color32::from_white_alpha(80));
        }

        // Events
        if response.hovered() {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::Grab);
        }

        if resized {
            let _ = tx.push(GuiToPlayerMsg::ResizeClip(
                self.id.clone(),
                self.trim_start,
                self.trim_end,
            ));
            let _ = tx.push(GuiToPlayerMsg::MoveClip(
                self.id.clone(),
                track_id,
                self.position,
            ));
        }

        response
    }

    pub fn trim_start_at(&mut self, beats: f32, bpm: f32) {
        let duration = self.audio.duration.unwrap().as_secs_f32() * bpm / 60.;
        self.trim_start += (beats - self.position) / duration;
        self.position = beats;
        self.trim_start = self.trim_start.clamp(0., self.trim_end - MIN_TRIM_RATIO);
    }

    pub fn trim_end_at(&mut self, beats: f32, bpm: f32) {
        let duration = self.audio.duration.unwrap().as_secs_f32() * bpm / 60.;
        self.trim_end = (beats - self.position) / duration + self.trim_start;
        self.trim_end = self.trim_end.clamp(self.trim_start + MIN_TRIM_RATIO, 1.);
    }

    fn left_resize_handle(
        &mut self,
        ui: &mut Ui,
        grid: &WorkspaceGrid,
        rect: Rect,
        bpm: f32,
    ) -> Response {
        let response = ui.allocate_rect(rect, Sense::drag());

        if response.dragged() {
            let delta = response.drag_delta().x;

            let prev_val = self.trim_start;
            self.trim_start += delta / grid.duration_to_width(self.audio.duration.unwrap(), bpm);
            self.trim_start = self.trim_start.clamp(0., self.trim_end - MIN_TRIM_RATIO);

            self.position +=
                (self.trim_start - prev_val) * self.audio.duration.unwrap().as_secs_f32() / 60.
                    * bpm
        }

        if response.hovered() {
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
    ) -> Response {
        let response = ui.allocate_rect(rect, Sense::drag());

        if response.dragged() {
            let delta = response.drag_delta().x;

            self.trim_end += delta / grid.duration_to_width(self.audio.duration.unwrap(), bpm);
            self.trim_end = self.trim_end.clamp(self.trim_start + MIN_TRIM_RATIO, 1.);
        }

        if response.hovered() {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
        }

        response
    }

    pub fn clone_with_new_id(&self) -> Self {
        let mut clone = self.clone();
        clone.id = uuid::Uuid::new_v4().to_string();
        clone
    }
}
