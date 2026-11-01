//! The project in miniature, above the options.

use egui::{Align2, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use tonique_engine::time::BeatPos;

use crate::{
    core::{
        export::ExportRange,
        state::ProjectState,
        track::{TrackKind, TrackRow},
    },
    ui::{
        theme::{ThemeExt, with_alpha},
        waveform::paint_waveform,
    },
};

use super::{LANE_PADDING, Selection, format::duration, widgets::card};

const PREVIEW_HEIGHT: f32 = 96.;
const LANE_GAP: f32 = 2.;
const MAX_LANE_HEIGHT: f32 = 16.;
/// Under the lanes, for the times.
const LABEL_STRIP: f32 = 10.;

/// The project in miniature: each track's clips in their colour with their
/// waveform, the tracks and the part of the timeline left out dimmed, and
/// the exported part's bounds marked. Dragging across it picks a custom
/// range, returned while dragging.
pub(super) fn arrangement_preview(
    ui: &mut Ui,
    state: &ProjectState,
    rows: &[TrackRow],
    selection: &Selection,
    range: ExportRange,
) -> Option<(BeatPos, BeatPos)> {
    let theme = ui.app_theme();
    let (rect, painter, response) = card(ui, PREVIEW_HEIGHT, &theme, Sense::drag());
    let project = state.export_span(ExportRange::Project).ok();
    let looped = state.export_span(ExportRange::Loop).ok();
    let chosen = state.export_span(range).ok();
    let shown = [project, looped, chosen]
        .into_iter()
        .flatten()
        .map(|s| s.1.0)
        .fold(0., f64::max);
    if project.is_none() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Nothing to export yet: add clips to the timeline",
            FontId::proportional(12.),
            theme.text_muted,
        );
        return None;
    }
    // Lanes on top, the exported part's start and end times below.
    let inner = rect
        .shrink(LANE_PADDING)
        .with_max_y(rect.bottom() - LANE_PADDING - LABEL_STRIP);
    let x = |beat: BeatPos| inner.left() + (beat.0 / shown) as f32 * inner.width();
    let bpm = state.bpm();

    // From where the drag started to the pointer, in whole beats.
    let beat_at = |pos: Pos2| {
        let beats = (pos.x - inner.left()) as f64 / inner.width() as f64 * shown;
        BeatPos(beats.round().clamp(0., shown.ceil()))
    };
    let anchor = response.id.with("anchor");
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        ui.data_mut(|d| d.insert_temp(anchor, beat_at(pos)));
    }
    let dragged = response
        .interact_pointer_pos()
        .filter(|_| response.dragged())
        .zip(ui.data(|d| d.get_temp::<BeatPos>(anchor)))
        .map(|(pos, from)| {
            let to = beat_at(pos);
            (BeatPos(from.0.min(to.0)), BeatPos(from.0.max(to.0)))
        })
        .filter(|(start, end)| end.0 > start.0);
    response
        .on_hover_cursor(CursorIcon::ResizeHorizontal)
        .on_hover_text("Drag across to export part of the project");

    let lanes: Vec<_> = rows
        .iter()
        .filter(|track| track.kind == TrackKind::Audio && !track.clips.is_empty())
        .collect();
    let lane_height = ((inner.height() - LANE_GAP * (lanes.len() as f32 - 1.))
        / lanes.len() as f32)
        .clamp(1., MAX_LANE_HEIGHT);
    for (i, track) in lanes.iter().enumerate() {
        let top = inner.top() + i as f32 * (lane_height + LANE_GAP);
        let color = if !selection.includes(state, track) {
            track.color.gamma_multiply(0.3)
        } else {
            track.color
        };
        let ink = with_alpha(theme.text_on(color), 170);
        for clip in &track.clips {
            let clip_rect =
                Rect::from_x_y_ranges(x(clip.position)..=x(clip.end(bpm)), top..=top + lane_height);
            painter.rect_filled(clip_rect, 1.5, color);
            let seconds = clip.source_seconds() as f64;
            if lane_height >= 6.
                && seconds > 0.
                && let Some(frames) = clip.audio.total_frames()
            {
                // Each pass of the clip's loop shows its part of the whole
                // file laid out.
                let pixels_per_beat = inner.width() / shown as f32;
                let file_width = (seconds * bpm as f64 / 60.) as f32 * pixels_per_beat;
                let clip_x = |beats: f64| clip_rect.left() + beats as f32 * pixels_per_beat;
                // Only the passes in the overview: a short loop makes many.
                let beats = |x: f32| ((x - clip_rect.left()) / pixels_per_beat) as f64;
                let in_view = beats(inner.left()).max(0.)..beats(inner.right());
                for segment in clip.segments(in_view, bpm) {
                    let (left, right) = (clip_x(segment.start), clip_x(segment.end));
                    let file_left = left - (segment.file_start / seconds) as f32 * file_width;
                    let file_rect =
                        Rect::from_min_size(pos2(file_left, top), vec2(file_width, lane_height));
                    let segment_rect = Rect::from_x_y_ranges(left..=right, clip_rect.y_range());
                    paint_waveform(
                        &painter,
                        file_rect,
                        segment_rect.intersect(clip_rect),
                        &clip.audio.data,
                        0.0..frames,
                        false,
                        ink,
                        clip.gain,
                        clip.pan,
                    );
                }
            }
        }
    }

    // What's exported stays bright; the rest goes under a veil.
    let marker = theme.accent;
    match state.export_span(range) {
        Ok((start, end)) => {
            let (left, right) = (x(start).max(rect.left()), x(end).min(rect.right()));
            let veil = |from: f32, to: f32| {
                if to > from {
                    painter.rect_filled(
                        Rect::from_x_y_ranges(from..=to, rect.y_range()),
                        0.,
                        with_alpha(theme.bg_deep, 190),
                    );
                }
            };
            veil(rect.left(), left);
            veil(right, rect.right());
            for edge in [left, right] {
                painter.line_segment(
                    [pos2(edge, rect.top()), pos2(edge, rect.bottom())],
                    Stroke::new(1.5, marker),
                );
            }
            let label = |pos: f32, align: Align2, text: String| {
                painter.text(
                    pos2(pos, rect.bottom() - 2.),
                    align,
                    text,
                    FontId::monospace(9.),
                    marker,
                );
            };
            let tempo_seconds = |beat: BeatPos| beat.0 * 60. / bpm as f64;
            label(
                left + 3.,
                Align2::LEFT_BOTTOM,
                duration(tempo_seconds(start)),
            );
            label(
                right - 3.,
                Align2::RIGHT_BOTTOM,
                duration(tempo_seconds(end)),
            );
        }
        Err(_) => {
            painter.rect_filled(rect, 4., with_alpha(theme.bg_deep, 190));
        }
    }
    dragged
}
