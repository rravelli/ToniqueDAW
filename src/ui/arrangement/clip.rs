use crate::ui::theme::font_size;
use crate::{
    core::{clip::AudioClip, state::ProjectState, track::TRACK_COLLAPSED_HEIGHT},
    ui::arrangement::grid::PaintGrid,
    ui::{
        theme::{ThemeExt, with_alpha},
        waveform::paint_waveform,
        widget::context_menu::ContextMenuButton,
    },
};
use egui::{
    Align2, Color32, CursorIcon, FontFamily, FontId, Painter, Pos2, Rect, Response, Sense, Stroke,
    Ui, Vec2, pos2, vec2,
};
use egui_phosphor::fill::TRASH;
use std::time::Duration;

const PADDING_TEXT: f32 = 4.;
const BORDER_WIDTH: f32 = 2.;
const HEADER_HEIGHT: f32 = 16.;
/// Width of the trim handles inside each edge of a clip.
const TRIM_HANDLE_WIDTH: f32 = 7.;
const MIN_TRIM_HANDLE_WIDTH: f32 = 2.;
/// Opacity of a silenced clip's body, header and outline over the lane: a
/// faint wash of its track's colour.
const SILENCED_BODY_ALPHA: u8 = 40;
const SILENCED_HEADER_ALPHA: u8 = 70;
const SILENCED_BORDER_ALPHA: u8 = 110;
#[derive(Clone, Copy)]
enum Edge {
    Start,
    End,
}

const LOADING_REPAINT: Duration = Duration::from_millis(100);
/// A clip on the timeline: a header to grab it by, trim handles at its
/// edges, and its waveform. Shows it, and applies trims to `state`.
pub struct ClipView<'a> {
    pub clip: &'a AudioClip,
    /// Where the whole clip is on screen; drawn clipped to `viewport`.
    pub rect: Rect,
    pub viewport: Rect,
    /// Its track's.
    pub color: Color32,
    pub selected: bool,
    /// False in a collapsed row: only the header shows.
    pub show_waveform: bool,
    /// Its track is muted or soloed out: drawn faded, in muted ink.
    pub silenced: bool,
}

impl ClipView<'_> {
    pub fn ui(&self, ui: &mut Ui, state: &mut ProjectState) -> Response {
        let (clip, viewport, color, selected, show_waveform) = (
            self.clip,
            self.viewport,
            self.color,
            self.selected,
            self.show_waveform,
        );
        let (pos, size) = (self.rect.min, self.rect.size());
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
        // Name and waveform are drawn on the clip's colour; a silenced clip
        // only keeps a trace of it, so it recedes without losing its track.
        let (fill, header, ink, outline) = if self.silenced {
            (
                with_alpha(fill, SILENCED_BODY_ALPHA),
                with_alpha(color, SILENCED_HEADER_ALPHA),
                theme.text_muted,
                with_alpha(color, SILENCED_BORDER_ALPHA),
            )
        } else {
            (fill, color, theme.text_on(fill), fill)
        };

        let stroke = if selected {
            Stroke::new(BORDER_WIDTH, theme.clip_selected)
        } else {
            Stroke::new(BORDER_WIDTH, outline)
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
        painter.rect(hitbox, 2.0, header, Stroke::NONE, egui::StrokeKind::Inside);

        let response = ui.interact(
            hitbox,
            format!("{:?}{}", clip.id, clip.position.0).into(),
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
            clip.audio.name.clone().to_string(),
            FontId::new(font_size::SMALL, FontFamily::Monospace),
            ink,
        );

        painter.line(
            vec![hitbox.left_bottom(), hitbox.right_bottom()],
            Stroke::new(1.0, with_alpha(ink, 60)),
        );
        // Waveform
        if show_waveform {
            let waveform_rect = Rect::from_min_max(
                pos2(pos.x, pos.y + HEADER_HEIGHT),
                pos2(pos.x + size.x, pos.y + size.y),
            );
            paint_clip_waveform(
                &painter,
                clip,
                waveform_rect,
                viewport,
                state.grid.pixels_per_beat(),
                state.bpm(),
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
            state.resize_clip(clip_copy.clone());
        }
        if drag_stopped {
            state.commit_resize_clip(clip_copy);
        }

        response
    }

    /// A trim handle: dragging it moves that edge of `clip`, snapping to
    /// the grid, other clips' edges, the loop and the edit cursor (Alt
    /// disables snapping).
    fn trim_handle(
        &self,
        ui: &mut Ui,
        state: &ProjectState,
        rect: Rect,
        viewport: Rect,
        clip: &mut AudioClip,
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

    fn context_menu(&self, ui: &mut Ui, clip: &AudioClip, state: &mut ProjectState) {
        ui.vertical(|ui| {
            if ui
                .add(ContextMenuButton::new(TRASH, "Delete").text_color(ui.app_theme().danger))
                .clicked()
            {
                state.delete_clips(&[clip.id]);
            };
        });
    }
}

/// The waveform of `clip` over `rect` (the clip's, from its start), as it
/// plays: piece by piece, each pass of its loop after a thin line marking
/// the seam. Each piece lays out the whole file and shows only its part, so
/// trimming doesn't shift which samples fall in each column.
pub fn paint_clip_waveform(
    painter: &Painter,
    clip: &AudioClip,
    rect: Rect,
    viewport: Rect,
    pixels_per_beat: f32,
    bpm: f32,
    color: Color32,
) {
    let Some(frames) = clip.audio.total_frames() else {
        return;
    };
    let seconds = clip.source_seconds() as f64;
    if seconds <= 0. {
        return;
    }
    let x = |beats: f64| rect.left() + beats as f32 * pixels_per_beat;
    let file_width = (seconds * bpm as f64 / 60.) as f32 * pixels_per_beat;
    for segment in clip.segments(bpm) {
        let (left, right) = (x(segment.start), x(segment.end));
        if right < viewport.left() || left > viewport.right() {
            continue;
        }
        let file_left = left - (segment.file_start / seconds) as f32 * file_width;
        let file =
            Rect::from_min_size(pos2(file_left, rect.top()), vec2(file_width, rect.height()));
        let shown = Rect::from_x_y_ranges(left..=right, rect.y_range());
        paint_waveform(
            painter,
            file,
            shown.intersect(rect).intersect(viewport),
            &clip.audio.data,
            0.0..frames,
            clip.audio.channels >= 2,
            color,
            clip.gain,
        );
        if segment.start > 0. {
            painter.with_clip_rect(viewport).vline(
                left,
                rect.y_range(),
                Stroke::new(1., with_alpha(color, 110)),
            );
        }
    }
}
