//! The project in miniature, above the options.

use crate::model::HoldsProject;
use gpui::{
    App, ContentMask, Context, CursorStyle, MouseButton, MouseDownEvent, Window, canvas,
    prelude::*, px,
};
use tonique_engine::time::BeatPos;

use crate::{
    app::export::ExportDialog,
    geometry::Rect,
    theme::{Color, Theme, hsla, mono, with_alpha},
    wave::paint_waveform,
    widgets::{paint_line, shape},
};
use tonique_core::core::{
    clip::AudioClip,
    export::{ExportRange, format::duration},
    state::ProjectState,
    track::{TrackKind, TrackRow},
};

use super::{LANE_PADDING, parts::card};

const PREVIEW_HEIGHT: f32 = 96.;
const LANE_GAP: f32 = 2.;
const MAX_LANE_HEIGHT: f32 = 16.;
/// Under the lanes, for the times.
const LABEL_STRIP: f32 = 10.;
const LABEL_SIZE: f32 = 9.;
/// The veil over what's left out.
const VEIL_ALPHA: u8 = 190;

/// How many beats the preview spans: to the furthest end of the project,
/// the loop and the range chosen.
fn preview_beats(state: &ProjectState, range: ExportRange) -> f64 {
    [ExportRange::Project, ExportRange::Loop, range]
        .into_iter()
        .filter_map(|range| state.export_span(range).ok())
        .map(|span| span.1.0)
        .fold(0., f64::max)
}

/// Where the lanes are in the preview's `rect`.
fn lanes_rect(rect: Rect) -> Rect {
    Rect::new(
        rect.left + LANE_PADDING,
        rect.top + LANE_PADDING,
        rect.width - 2. * LANE_PADDING,
        rect.height - 2. * LANE_PADDING - LABEL_STRIP,
    )
}

impl ExportDialog {
    /// The whole beat under `pointer_x` in the preview, if it shows the
    /// project.
    pub(super) fn preview_beat(&self, pointer_x: f32, context: &App) -> Option<BeatPos> {
        let shown = preview_beats(self.state(context), self.options.range);
        let inner = lanes_rect(Rect::of(self.preview_bounds.get()));
        if shown <= 0. || inner.width <= 0. {
            return None;
        }
        let beats = (pointer_x - inner.left) as f64 / inner.width as f64 * shown;
        Some(BeatPos(beats.round().clamp(0., shown.ceil())))
    }

    /// The project in miniature: each track's clips in their colour with
    /// their waveform, the tracks and the part of the timeline left out
    /// dimmed, and the exported part's bounds marked. Dragging in it moves
    /// the bound nearest the pointer.
    pub(super) fn preview(
        &self,
        rows: &[TrackRow],
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let state = self.state(context);
        let frame = card(PREVIEW_HEIGHT, theme);
        if state.export_span(ExportRange::Project).is_err() {
            return frame
                .flex()
                .items_center()
                .justify_center()
                .text_color(hsla(theme.text_muted))
                .child("Nothing to export yet: add clips to the timeline");
        }
        let shown = preview_beats(state, self.options.range);
        let bpm = state.bpm();
        let chosen = state.export_span(self.options.range).ok();
        let lanes: Vec<(Color, Vec<AudioClip>)> = rows
            .iter()
            .filter(|track| track.kind == TrackKind::Audio && !track.clips.is_empty())
            .map(|track| {
                let color = if self.selection.includes(state, track) {
                    track.color
                } else {
                    with_alpha(track.color, (track.color.a as f32 * 0.3) as u8)
                };
                (color, track.clips.clone())
            })
            .collect();
        let slot = self.preview_bounds.clone();
        let waves = self.waves.clone();
        let theme = theme.clone();
        let painting = canvas(
            move |bounds, _, _| slot.set(bounds),
            move |bounds, _, window, app| {
                let rect = Rect::of(bounds);
                let inner = lanes_rect(rect);
                let pixels_per_beat = inner.width / shown as f32;
                let x = |beat: BeatPos| inner.left + beat.0 as f32 * pixels_per_beat;
                let lane_height = ((inner.height - LANE_GAP * (lanes.len() as f32 - 1.))
                    / lanes.len() as f32)
                    .clamp(1., MAX_LANE_HEIGHT);
                let mask = ContentMask { bounds };
                window.with_content_mask(Some(mask), |window| {
                    let mut waves = waves.borrow_mut();
                    for (index, (color, clips)) in lanes.iter().enumerate() {
                        let top = inner.top + index as f32 * (lane_height + LANE_GAP);
                        let ink = hsla(with_alpha(theme.text_on(*color), 170));
                        for clip in clips {
                            let left = x(clip.position);
                            let clip_rect =
                                Rect::new(left, top, x(clip.end(bpm)) - left, lane_height);
                            window.paint_quad(
                                gpui::fill(clip_rect.bounds(), hsla(*color)).corner_radii(px(1.5)),
                            );
                            if lane_height >= 6. {
                                paint_passes(
                                    window,
                                    &mut waves,
                                    clip,
                                    clip_rect,
                                    inner,
                                    pixels_per_beat,
                                    bpm,
                                    ink,
                                );
                            }
                        }
                    }
                });

                // What's exported stays bright; the rest goes under a veil.
                let veil = hsla(with_alpha(theme.bg_deep, VEIL_ALPHA));
                let Some((start, end)) = chosen else {
                    window.paint_quad(gpui::fill(bounds, veil).corner_radii(px(4.)));
                    return;
                };
                let (left, right) = (x(start).max(rect.left), x(end).min(rect.right()));
                for (from, to) in [(rect.left, left), (right, rect.right())] {
                    if to > from {
                        let part = Rect::new(from, rect.top, to - from, rect.height);
                        window.paint_quad(gpui::fill(part.bounds(), veil));
                    }
                }
                let marker = hsla(theme.accent);
                for edge in [left, right] {
                    let line = Rect::new(edge - 0.75, rect.top, 1.5, rect.height);
                    window.paint_quad(gpui::fill(line.bounds(), marker));
                }
                let seconds = |beat: BeatPos| beat.0 * 60. / bpm as f64;
                let top = rect.bottom() - 2. - LABEL_SIZE * 1.25;
                let start_label =
                    shape(window, duration(seconds(start)), LABEL_SIZE, marker, mono());
                paint_line(window, app, &start_label, left + 3., top, LABEL_SIZE);
                let end_label = shape(window, duration(seconds(end)), LABEL_SIZE, marker, mono());
                let end_left = right - 3. - f32::from(end_label.width);
                paint_line(window, app, &end_label, end_left, top, LABEL_SIZE);
            },
        )
        .size_full();
        frame
            .cursor(CursorStyle::ResizeLeftRight)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(|this, event: &MouseDownEvent, _, context| {
                    context.stop_propagation();
                    this.preview_down(event, context);
                    context.notify();
                }),
            )
            .child(painting)
    }
}

/// `clip`'s waveform over `clip_rect`, each pass of its loop showing its
/// part of the whole file laid out, as one lane.
#[allow(clippy::too_many_arguments)]
fn paint_passes(
    window: &mut Window,
    waves: &mut crate::wave::WaveCache,
    clip: &AudioClip,
    clip_rect: Rect,
    view: Rect,
    pixels_per_beat: f32,
    bpm: f32,
    ink: gpui::Hsla,
) {
    let seconds = clip.source_seconds() as f64;
    let Some(frames) = clip.audio.total_frames() else {
        return;
    };
    if seconds <= 0. {
        return;
    }
    // The whole file as the clip plays it: at its speed.
    let file_width = clip.source_beats(bpm) as f32 * pixels_per_beat;
    let clip_x = |beats: f64| clip_rect.left + beats as f32 * pixels_per_beat;
    // Only the passes in the overview: a short loop makes many.
    let beats = |x: f32| ((x - clip_rect.left) / pixels_per_beat) as f64;
    let in_view = beats(view.left).max(0.)..beats(view.right());
    for segment in clip.segments(in_view, bpm) {
        let (left, right) = (clip_x(segment.start), clip_x(segment.end));
        let file_left = left - (segment.file_start / seconds) as f32 * file_width;
        let file = Rect::new(file_left, clip_rect.top, file_width, clip_rect.height);
        let shown =
            Rect::new(left, clip_rect.top, right - left, clip_rect.height).intersect(clip_rect);
        paint_waveform(
            window,
            waves,
            file,
            shown,
            &clip.audio.data,
            0.0..frames,
            false,
            ink,
            clip.gain,
            clip.pan,
        );
    }
}
