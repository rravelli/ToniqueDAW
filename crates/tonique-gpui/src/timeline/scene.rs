//! What the timeline shows this frame, taken out of the state so the
//! canvas can paint it after the view is built.

use crate::model::HoldsProject;
use crate::timeline::Arrangement;
use gpui::App;
use std::collections::HashSet;

use tonique_engine::{edit::ClipId, time::BeatPos};

use crate::{
    geometry::Rect,
    theme::{Color, Theme, with_alpha},
    timeline::{
        FOOTER_PADDING, RULER_HEIGHT, RowLayout, footer_height,
        input::TimelineDrag,
        moving::{ClipMove, DraggedClips},
        rows::{ClipPart, HEADER_HEIGHT, Span, handle_rect},
        ruler::LoopGrab,
    },
};
use tonique_core::{
    core::{
        clip::AudioClip,
        grid::GridLine,
        state::MASTER_TRACK_ID,
        track::{DEFAULT_TRACK_HEIGHT, TRACK_COLLAPSED_HEIGHT, TrackRow},
    },
    utils::format_time,
};

/// Lines and labels are taken this far past the right edge, in case the
/// view grew since it was measured.
const OVERSCAN: f32 = 400.;
/// How opaque the clips in a group's overview are, and those of a
/// silenced track in it.
const OVERVIEW_ALPHA: u8 = 80;
const SILENCED_OVERVIEW_ALPHA: u8 = 30;
/// Between a group's lane's top and its overview, and below it.
const OVERVIEW_INSET: f32 = 2.;

pub struct SceneRow {
    pub top: f32,
    pub height: f32,
    pub color: Color,
    pub selected: bool,
}

pub struct SceneClip {
    pub clip: AudioClip,
    /// Local to the timeline.
    pub rect: Rect,
    pub color: Color,
    /// Where it's selected across, local to the timeline: all of it, or
    /// its part in the selection zone.
    pub selected: Option<(f32, f32)>,
    /// Its header under the pointer.
    pub lit: bool,
    /// What a drag leaves in its place until dropped: faded, its waveform
    /// black.
    pub left_behind: bool,
    pub silenced: bool,
    pub collapsed: bool,
}

/// The ruler's marks, local to the timeline.
#[derive(Default)]
pub struct Marks {
    /// x, and whether it's a bar line.
    pub lines: Vec<(f32, bool)>,
    /// x, text, and whether it's a bar's.
    pub labels: Vec<(f32, String, bool)>,
    pub times: Vec<(f32, String)>,
}

pub struct Scene {
    pub theme: std::rc::Rc<Theme>,
    pub pixels_per_beat: f32,
    pub bpm: f32,
    pub rows: Vec<SceneRow>,
    /// What's inside the groups, on their lanes: a faint bar per clip,
    /// local to the timeline.
    pub overviews: Vec<(Rect, Color)>,
    pub clips: Vec<SceneClip>,
    /// The clip a drop would add.
    pub drop: Option<SceneClip>,
    /// Lanes of the tracks a drop would add below the last: their tops,
    /// local to the timeline, and colours.
    pub new_lanes: Vec<(f32, Color)>,
    /// The selection zone, local to the timeline.
    pub zone: Option<Rect>,
    /// What dragged clips snapped to: x, local to the timeline.
    pub snap_line: Option<f32>,
    /// Over a clip's body: where a click would place the edit cursor, and
    /// the body.
    pub cursor_hint: Option<(f32, Rect)>,
    /// The trim handle under the pointer or dragged.
    pub lit_handle: Option<Rect>,
    /// The lane an effect would be dropped on, local to the timeline: its
    /// top and height.
    pub effect_drop: Option<(f32, f32)>,
    pub marks: Marks,
    pub playhead: f32,
    pub edit_cursor: f32,
    pub looping: bool,
    pub loop_x: (f32, f32),
    /// The loop's part under the pointer or dragged.
    pub loop_lit: Option<LoopGrab>,
    /// The playhead's head is under the pointer or dragged.
    pub playhead_lit: bool,
    /// The footer's height, holding the master.
    pub footer_height: f32,
    /// The master's output, oldest first.
    pub scope: Vec<[f32; 2]>,
}

fn label(line: GridLine) -> String {
    match line {
        GridLine::Bar(number) => format!("{number}"),
        GridLine::Beat { bar, beat } => format!("{bar}.{beat}"),
        GridLine::Division {
            bar,
            beat,
            sub: Some(division),
        } => format!("{bar}.{beat}.{division}"),
        GridLine::Division {
            bar,
            beat,
            sub: None,
        } => format!("{bar}.{beat}"),
    }
}

impl Arrangement {
    /// What the timeline paints: the clips of the rows overlapping
    /// `in_view`, a span of the scrolled content.
    pub(super) fn scene(
        &self,
        rows: &RowLayout,
        in_view: (f32, f32),
        theme: std::rc::Rc<Theme>,
        context: &App,
    ) -> Scene {
        let state = self.state(context);
        let screen_x = |beat: BeatPos| state.grid.beats_to_x(beat, self.viewport());
        let (loop_start, loop_end) = state.loop_range();
        let (scene_rows, mut clips) = self.scene_clips(rows, in_view, &theme, context);
        let overviews = rows
            .iter()
            .filter(|row| row.is_group() && row.overlaps(in_view))
            .flat_map(|row| self.group_overview(row, context))
            .collect();
        clips.extend(self.scene_dragged(rows, &theme, context));
        let dragged = self.dragged_clips();
        Scene {
            theme: theme.clone(),
            pixels_per_beat: state.grid.pixels_per_beat(),
            bpm: state.bpm(),
            rows: scene_rows,
            overviews,
            clips,
            drop: self.scene_drop(rows, &theme, context),
            new_lanes: self.scene_new_lanes(rows, context),
            zone: self.scene_zone(rows, context),
            snap_line: dragged.and_then(|dragged| dragged.snapped_to).map(screen_x),
            cursor_hint: self.cursor_hint(rows, context),
            lit_handle: self.lit_handle(rows, context),
            effect_drop: self.scene_effect_drop(rows, context),
            marks: self.marks(context),
            playhead: screen_x(state.playhead()),
            edit_cursor: screen_x(state.edit_cursor()),
            looping: state.looping(),
            loop_x: (screen_x(loop_start), screen_x(loop_end)),
            loop_lit: self.loop_lit(context),
            playhead_lit: self.playhead_lit(context),
            footer_height: footer_height(state),
            scope: state
                .metrics()
                .tracks
                .get(&MASTER_TRACK_ID)
                .map(|metrics| metrics.scope().iter().copied().collect())
                .unwrap_or_default(),
        }
    }

    /// The grid's lines, the bars' labels and the times, in view.
    fn marks(&self, context: &App) -> Marks {
        let grid = &self.state(context).grid;
        let viewport = self.viewport();
        let wide = Span {
            right: viewport.right + OVERSCAN,
            ..viewport
        };
        let (from, to) = grid.visible_beats(wide);
        let screen_x = |beat: BeatPos| grid.beats_to_x(beat, viewport);
        let is_bar = |line: &GridLine| matches!(line, GridLine::Bar(_));
        let lines = grid
            .lines(from, to, grid.line_step())
            .into_iter()
            .map(|(beat, line)| (screen_x(beat), is_bar(&line)))
            .collect();
        let labels = grid
            .lines(from, to, grid.label_step(from, to))
            .into_iter()
            .map(|(beat, line)| (screen_x(beat), label(line), is_bar(&line)))
            .collect();

        let bpm = self.state(context).bpm();
        let step = grid.time_step(bpm);
        let seconds_at = |beat: BeatPos| beat.0 * 60. / bpm as f64;
        let first = (seconds_at(from).max(0.) / step).floor() as i64;
        let last = (seconds_at(to) / step).ceil() as i64;
        let times = (first..=last)
            .map(|index| {
                let seconds = index as f64 * step;
                let mut text = format_time(seconds);
                // Whole seconds: without the milliseconds.
                if step >= 1. {
                    text.truncate(text.len() - 4);
                }
                (screen_x(BeatPos(seconds * bpm as f64 / 60.)), text)
            })
            .collect();
        Marks {
            lines,
            labels,
            times,
        }
    }

    /// The clips being dragged, once they are.
    fn dragged_clips(&self) -> Option<&DraggedClips> {
        match &self.timeline.drag {
            TimelineDrag::Move(ClipMove {
                dragged: Some(dragged),
                ..
            }) => Some(dragged),
            _ => None,
        }
    }

    /// The rows, and their clips: one being trimmed over its neighbours.
    fn scene_clips(
        &self,
        rows: &RowLayout,
        in_view: (f32, f32),
        theme: &Theme,
        context: &App,
    ) -> (Vec<SceneRow>, Vec<SceneClip>) {
        let state = self.state(context);
        let left_behind: HashSet<ClipId> = self
            .dragged_clips()
            .filter(|dragged| !dragged.duplicate)
            .map(|dragged| dragged.ids().into_iter().collect())
            .unwrap_or_default();
        let lit = self.lit_header(rows, context);
        let mut scene_rows = Vec::new();
        let mut clips = Vec::new();
        for row in rows.iter() {
            let track = &row.track;
            scene_rows.push(SceneRow {
                top: self.row_top(row, context),
                height: track.height,
                color: track.color,
                selected: track.selected,
            });
            // A group's clips are its tracks': shown in its overview.
            if row.is_group() || !row.overlaps(in_view) {
                continue;
            }
            let mut shown: Vec<AudioClip> = track
                .clips
                .iter()
                .map(|clip| state.shown_clip(clip))
                .collect();
            shown.sort_by_key(|clip| state.is_resizing(clip.id));
            for clip in shown {
                let behind = left_behind.contains(&clip.id);
                clips.push(SceneClip {
                    lit: lit == Some(clip.id) && !behind,
                    rect: self.clip_rect(&clip, row, context),
                    color: if behind {
                        theme.hover_overlay
                    } else {
                        track.color
                    },
                    selected: (!behind && state.is_clip_selected(clip.id))
                        .then(|| self.selected_span(&clip, context))
                        .flatten(),
                    left_behind: behind,
                    silenced: track.is_silenced(),
                    collapsed: track.collapsed,
                    clip,
                });
            }
        }
        (scene_rows, clips)
    }

    /// What's inside a group, in a band as tall as a collapsed row at the
    /// top of its lane: a strip per track in it, in order, with that
    /// track's clips in its colour, faint so they don't pass for clips.
    fn group_overview(&self, row: &crate::timeline::Row, context: &App) -> Vec<(Rect, Color)> {
        let state = self.state(context);
        let tracks = state.group_tracks(row.track.id);
        if tracks.is_empty() {
            return Vec::new();
        }
        let viewport = self.viewport();
        let top = self.row_top(row, context) + OVERVIEW_INSET;
        let band = (TRACK_COLLAPSED_HEIGHT.min(row.track.height) - 2. * OVERVIEW_INSET).max(0.);
        let strip = band / tracks.len() as f32;
        let bpm = state.bpm();
        let mut bars = Vec::new();
        for (index, track) in tracks.iter().enumerate() {
            let alpha = if track.is_silenced() {
                SILENCED_OVERVIEW_ALPHA
            } else {
                OVERVIEW_ALPHA
            };
            let color = with_alpha(track.color, alpha);
            let strip_top = top + index as f32 * strip;
            for clip in &track.clips {
                // Only what's on screen: zoomed in, a clip can be millions
                // of pixels wide.
                let left = state.grid.beats_to_x(clip.position, viewport).max(-1.);
                let right = state
                    .grid
                    .beats_to_x(clip.end(bpm), viewport)
                    .min(viewport.right + 1.);
                if right > left {
                    bars.push((Rect::new(left, strip_top, right - left, strip), color));
                }
            }
        }
        bars
    }

    /// Where a selected clip is lit across: its part in the zone, or all
    /// of it.
    fn selected_span(&self, clip: &AudioClip, context: &App) -> Option<(f32, f32)> {
        let state = self.state(context);
        let screen_x = |beat: BeatPos| state.grid.beats_to_x(beat, self.viewport());
        let end = clip.end(state.bpm());
        let (start, stop) = match state.selection_bounds() {
            Some(zone) => (zone.start_pos.max(clip.position), zone.end_pos.min(end)),
            None => (clip.position, end),
        };
        (stop > start).then(|| (screen_x(start), screen_x(stop)))
    }

    /// The dragged clips where they'd land, selected, over everything; on
    /// the tracks a drop would add, in the colours they'll get.
    fn scene_dragged(&self, rows: &RowLayout, theme: &Theme, context: &App) -> Vec<SceneClip> {
        let Some(dragged) = self.dragged_clips() else {
            return Vec::new();
        };
        let tracks: Vec<TrackRow> = self.state(context).tracks().collect();
        let colors = self
            .state(context)
            .new_track_colors(dragged.added_tracks(tracks.len()));
        dragged
            .clips
            .iter()
            .map(|dragged_clip| {
                let index = dragged_clip.track_index;
                let (top, height) = self.track_span(rows, index, context);
                let target = tracks.get(index);
                let color = target.map_or_else(
                    || {
                        colors
                            .get(index - tracks.len())
                            .copied()
                            .unwrap_or(theme.text_muted)
                    },
                    |track| track.color,
                );
                let clip = dragged_clip.clip.clone();
                let rect = self.clip_rect_at(&clip, top, height, context);
                SceneClip {
                    selected: Some((rect.left, rect.right())),
                    rect,
                    color,
                    lit: false,
                    left_behind: false,
                    silenced: target.is_some_and(|track| track.is_silenced()),
                    collapsed: target.is_some_and(|track| track.collapsed),
                    clip,
                }
            })
            .collect()
    }

    /// The lanes of the tracks a drag of clips or audio would add.
    fn scene_new_lanes(&self, rows: &RowLayout, context: &App) -> Vec<(f32, Color)> {
        let track_count = self.state(context).track_count();
        let count = match (self.dragged_clips(), &self.timeline.drop_target) {
            (Some(dragged), _) => dragged.added_tracks(track_count),
            (None, Some(target)) if target.track_index.is_none() => 1,
            _ => 0,
        };
        self.state(context)
            .new_track_colors(count)
            .into_iter()
            .enumerate()
            .map(|(index, color)| (self.track_span(rows, track_count + index, context).0, color))
            .collect()
    }

    /// The selection zone: from the top of its first track's row to the
    /// bottom of its last's (a collapsed group's, for a hidden track).
    fn scene_zone(&self, rows: &RowLayout, context: &App) -> Option<Rect> {
        let zone = self.state(context).selection_bounds()?;
        let (top, _) = self.track_span(rows, zone.start_track_index, context);
        let (last_top, last_height) = self.track_span(rows, zone.end_track_index, context);
        let viewport = self.viewport();
        let left = self
            .state(context)
            .grid
            .beats_to_x(zone.start_pos, viewport);
        let right = self.state(context).grid.beats_to_x(zone.end_pos, viewport);
        Some(Rect::new(
            left,
            top,
            right - left,
            last_top + last_height - top,
        ))
    }

    /// Over a clip's body while not dragging: where a click would place
    /// the edit cursor (snapped as it would be), and the body.
    fn cursor_hint(&self, rows: &RowLayout, context: &App) -> Option<(f32, Rect)> {
        let (TimelineDrag::None, Some((local_x, local_y))) =
            (&self.timeline.drag, self.timeline.hover)
        else {
            return None;
        };
        let (row, clip, ClipPart::Body) = self.clip_at(rows, local_x, local_y, context)? else {
            return None;
        };
        let rect = self.clip_rect(&clip, &rows[row], context);
        let body = Rect::new(
            rect.left,
            rect.top + HEADER_HEIGHT,
            rect.width,
            rect.height - HEADER_HEIGHT,
        );
        let alt = self.timeline.hover_alt;
        let beat = self
            .state(context)
            .grid
            .snap(self.beats_at(local_x, context), alt);
        Some((
            self.state(context).grid.beats_to_x(beat, self.viewport()),
            body,
        ))
    }

    /// The trim handle being dragged, or else under the pointer.
    fn lit_handle(&self, rows: &RowLayout, context: &App) -> Option<Rect> {
        if let TimelineDrag::Trim {
            original,
            start_edge,
            current,
        } = &self.timeline.drag
        {
            let clip = current.as_ref().unwrap_or(original);
            let (row, _) = self.state(context).clip_with_track(clip.id)?;
            let row = rows.iter().find(|shown| shown.track.id == row.id)?;
            return Some(handle_rect(self.clip_rect(clip, row, context), *start_edge));
        }
        let (TimelineDrag::None, Some((local_x, local_y))) =
            (&self.timeline.drag, self.timeline.hover)
        else {
            return None;
        };
        let (row, clip, part) = self.clip_at(rows, local_x, local_y, context)?;
        let start_edge = match part {
            ClipPart::TrimStart => true,
            ClipPart::TrimEnd => false,
            _ => return None,
        };
        Some(handle_rect(
            self.clip_rect(&clip, &rows[row], context),
            start_edge,
        ))
    }

    /// The clip whose header the pointer is over, while not dragging.
    fn lit_header(&self, rows: &RowLayout, context: &App) -> Option<ClipId> {
        let (TimelineDrag::None, Some((local_x, local_y))) =
            (&self.timeline.drag, self.timeline.hover)
        else {
            return None;
        };
        match self.clip_at(rows, local_x, local_y, context) {
            Some((_, clip, ClipPart::Header)) => Some(clip.id),
            _ => None,
        }
    }

    /// The lane an effect would be dropped on: a track's, or the master's
    /// in the footer, as high as its header.
    fn scene_effect_drop(&self, rows: &RowLayout, context: &App) -> Option<(f32, f32)> {
        let target = self.timeline.effect_target?;
        if target == MASTER_TRACK_ID {
            let master = self.state(context).master_track();
            return Some((self.lanes_bottom(context) + FOOTER_PADDING, master.height));
        }
        let row = rows.iter().find(|row| row.track.id == target)?;
        Some((self.row_top(row, context), row.track.height))
    }

    /// The clip a drop would add.
    fn scene_drop(&self, rows: &RowLayout, theme: &Theme, context: &App) -> Option<SceneClip> {
        let target = self.timeline.drop_target.as_ref()?;
        let state = self.state(context);
        let track = target
            .track_index
            .and_then(|index| state.track_from_index(index));
        let (top, height, color) = match (target.track_index, track) {
            (Some(index), Some(track)) => {
                let (top, height) = self.track_span(rows, index, context);
                (top, height, track.color)
            }
            _ => (
                RULER_HEIGHT + rows.height() - state.grid.offset.y,
                DEFAULT_TRACK_HEIGHT,
                state
                    .new_track_colors(1)
                    .first()
                    .copied()
                    .unwrap_or(theme.text_muted),
            ),
        };
        let clip = AudioClip::placed(
            ClipId(0),
            target.audio.clone(),
            target.position,
            state.bpm(),
        );
        let left = state.grid.beats_to_x(target.position, self.viewport());
        let rect = Rect::new(left, top, state.grid.beats_to_width(clip.length), height);
        let ghost = SceneClip {
            clip,
            rect,
            color,
            selected: None,
            lit: false,
            left_behind: false,
            silenced: false,
            collapsed: false,
        };
        Some(ghost)
    }
}
