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
/// How much of its track's colour a clip's body has, mixed into the
/// timeline's background: a tint, unlike its solid header, so the header
/// reads as the clip to grab and the body as time. Opaque, so clips dragged
/// over others hide them.
const BODY_TINT: f32 = 0.4;
/// A selected clip's body: a little more, its header carrying the selection.
const SELECTED_BODY_TINT: f32 = 0.5;
/// How much of its track's colour a silenced clip's body, header and
/// outline have: a faint wash of it.
const SILENCED_BODY_TINT: f32 = 0.16;
const SILENCED_HEADER_TINT: f32 = 0.27;
const SILENCED_BORDER_TINT: f32 = 0.43;
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
        // The header is the clip, to select and move: solid in its track's
        // colour, lit up when selected (towards a neutral colour that can't
        // clash with the clip's). The body is time, a click there places
        // the edit cursor: a tint of the colour, the waveform on it.
        let header = if selected {
            color.lerp_to_gamma(theme.clip_selected, 0.35)
        } else {
            color
        };
        // Mixed into the background rather than see-through.
        let tint = |color: Color32, amount: f32| theme.bg_base.lerp_to_gamma(color, amount);
        let (body, header, header_ink, wave, outline) = if self.silenced {
            (
                tint(color, SILENCED_BODY_TINT),
                tint(color, SILENCED_HEADER_TINT),
                theme.text_muted,
                theme.text_muted,
                tint(color, SILENCED_BORDER_TINT),
            )
        } else {
            let amount = if selected {
                SELECTED_BODY_TINT
            } else {
                BODY_TINT
            };
            (
                tint(header, amount),
                header,
                theme.text_on(header),
                color,
                tint(color, 0.67),
            )
        };
        let stroke = if selected {
            Stroke::new(BORDER_WIDTH, theme.clip_selected)
        } else {
            Stroke::new(1., outline)
        };
        // Main rect
        painter.rect(
            Rect::from_min_size(pos, size),
            2.0,
            body,
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
                .paint_clip_grid(&painter, viewport, rect, with_alpha(wave, 30));
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
        let response = ui.interact(
            hitbox,
            format!("{:?}{}", clip.id, clip.position.0).into(),
            Sense::all(),
        );
        // Lights up under the pointer, like a button.
        let header = if response.hovered() || response.dragged() {
            header.blend(theme.hover_overlay)
        } else {
            header
        };
        painter.rect(hitbox, 2.0, header, Stroke::NONE, egui::StrokeKind::Inside);
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
            header_ink,
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
                wave,
            );
            // Between the trim handles, the body is the timeline's: show
            // where a click places the edit cursor.
            let body = Rect::from_min_max(
                pos2(pos.x + handle_width, waveform_rect.top()),
                pos2(pos.x + size.x - handle_width, waveform_rect.bottom()),
            )
            .intersect(viewport);
            self.body_hover(ui, state, body);
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

    /// Over `body`, while nothing's dragged: a text cursor, and a line where
    /// a click places the edit cursor (snapped as it would be).
    fn body_hover(&self, ui: &Ui, state: &ProjectState, body: Rect) {
        let idle = ui.ctx().dragged_id().is_none() && !ui.input(|i| i.pointer.any_down());
        let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        if !idle || !body.contains(pointer) || !ui.rect_contains_pointer(body) {
            return;
        }
        ui.ctx().set_cursor_icon(CursorIcon::Text);
        let beats = state.grid.x_to_beats(pointer.x, self.viewport);
        let x = state
            .grid
            .beats_to_x(state.grid.snap_at_grid(beats), self.viewport);
        let theme = ui.app_theme();
        ui.painter_at(body).vline(
            x,
            body.y_range(),
            Stroke::new(1., with_alpha(theme.text, 170)),
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::AudioInfo;
    use tonique_engine::{
        edit::ClipId,
        engine::{Engine, EngineConfig},
        time::BeatPos,
    };

    /// The cursor over `at`, with a clip drawn over (0, 0)–(400, 60).
    fn cursor_at(at: Pos2) -> CursorIcon {
        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        let clip = AudioClip::new(ClipId(1), AudioInfo::silent(4.), BeatPos(0.), 120.);
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let mut cursor = CursorIcon::Default;
        for events in [vec![], vec![egui::Event::PointerMoved(at)]] {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800., 200.))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                ClipView {
                    clip: &clip,
                    rect: Rect::from_min_size(Pos2::ZERO, vec2(400., 60.)),
                    viewport: Rect::from_min_size(Pos2::ZERO, vec2(800., 200.)),
                    color: crate::ui::theme::Theme::dark().accent,
                    selected: false,
                    show_waveform: true,
                    silenced: false,
                }
                .ui(ui, &mut state);
            });
            output.textures_delta.clear();
            cursor = output.platform_output.cursor_icon;
        }
        cursor
    }

    #[test]
    fn the_header_grabs_the_clip_and_the_body_places_the_cursor() {
        assert_eq!(cursor_at(pos2(200., 8.)), CursorIcon::Grab, "header");
        assert_eq!(cursor_at(pos2(200., 40.)), CursorIcon::Text, "body");
        assert_eq!(
            cursor_at(pos2(398., 40.)),
            CursorIcon::ResizeHorizontal,
            "a trim handle"
        );
    }
}
