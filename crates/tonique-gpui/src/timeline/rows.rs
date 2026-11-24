//! Where things are on the timeline: its rows, the clips on them, and the
//! beats under the pointer.

use crate::model::HoldsProject;
use crate::timeline::Arrangement;
use gpui::{App, Pixels, Point, Window};
use tonique_engine::time::BeatPos;

use crate::{geometry::Rect, timeline::RULER_HEIGHT};
use tonique_core::core::{
    clip::AudioClip,
    grid::Viewport,
    row_layout::{Row, RowLayout},
    state::ProjectState,
    track::DEFAULT_TRACK_HEIGHT,
};

/// A clip's header: dragged to move it.
pub const HEADER_HEIGHT: f32 = 16.;
const TRIM_HANDLE: f32 = 7.;
const MIN_TRIM_HANDLE: f32 = 2.;

/// The timeline's extent across the screen, for Tonique's grid.
#[derive(Clone, Copy)]
pub struct Span {
    pub left: f32,
    pub right: f32,
}

impl Viewport for Span {
    fn left(&self) -> f32 {
        self.left
    }
    fn right(&self) -> f32 {
        self.right
    }
}

/// Above the master's header, below the footer's top line.
pub const FOOTER_PADDING: f32 = 3.;

/// The footer holding the master, below the rows of the timeline and of
/// the track list: as tall as the master's row.
pub fn footer_height(state: &ProjectState) -> f32 {
    state.master_track().height + FOOTER_PADDING
}

/// What the pointer is over, in a clip.
pub enum ClipPart {
    TrimStart,
    TrimEnd,
    Header,
    Body,
}

/// How wide a clip's trim handles are: narrower on narrow clips, leaving
/// their middle to grab them by.
fn handle_width(clip: Rect) -> f32 {
    (clip.width / 4.).clamp(MIN_TRIM_HANDLE, TRIM_HANDLE)
}

/// The trim handle inside a clip's start or end edge.
pub fn handle_rect(clip: Rect, start_edge: bool) -> Rect {
    let width = handle_width(clip);
    let left = if start_edge {
        clip.left
    } else {
        clip.right() - width
    };
    Rect::new(left, clip.top, width, clip.height)
}

impl Arrangement {
    /// The timeline across the screen, in its own coordinates.
    pub(super) fn viewport(&self) -> Span {
        Span {
            left: 0.,
            right: f32::from(self.timeline.bounds.get().size.width),
        }
    }

    pub(super) fn height(&self) -> f32 {
        f32::from(self.timeline.bounds.get().size.height)
    }

    /// Where the lanes end and the master's footer starts, local to the
    /// timeline.
    pub(super) fn lanes_bottom(&self, context: &App) -> f32 {
        (self.height() - footer_height(self.state(context))).max(RULER_HEIGHT)
    }

    /// Whether `local_y` is on the footer, below the lanes.
    pub(super) fn in_footer(&self, local_y: f32, context: &App) -> bool {
        local_y >= self.lanes_bottom(context)
    }

    /// A point in the window, local to the timeline.
    pub(super) fn local(&self, point: Point<Pixels>) -> (f32, f32) {
        let origin = self.timeline.bounds.get().origin;
        (f32::from(point.x - origin.x), f32::from(point.y - origin.y))
    }

    /// The span of the rows in view, in the scrolled content: from the
    /// lanes' top down by the window's height, more than the lanes ever
    /// show, so it holds before the panels are laid out at a new size.
    pub fn rows_in_view(&self, window: &Window, context: &App) -> (f32, f32) {
        let top = self.state(context).grid.offset.y;
        (top, top + f32::from(window.viewport_size().height))
    }

    /// A row's top, local to the timeline.
    pub(super) fn row_top(&self, row: &Row, context: &App) -> f32 {
        RULER_HEIGHT + row.top - self.state(context).grid.offset.y
    }

    /// The row at `local_y`, if it's on one shown (not under the footer).
    pub(super) fn row_at(&self, rows: &RowLayout, local_y: f32, context: &App) -> Option<usize> {
        if self.in_footer(local_y, context) {
            return None;
        }
        rows.row_at(self.content_y(local_y, context))
    }

    /// A height local to the timeline, in the scrolled content.
    pub(super) fn content_y(&self, local_y: f32, context: &App) -> f32 {
        local_y - RULER_HEIGHT + self.state(context).grid.offset.y
    }

    /// The track under `local_y`, in the engine's order: on a group's
    /// header, its first (clips always land on tracks).
    pub(super) fn track_at(&self, rows: &RowLayout, local_y: f32, context: &App) -> Option<usize> {
        self.row_at(rows, local_y, context)
            .map(|row| rows[row].track.first_track_index)
    }

    /// The track under `local_y`, for a drag: above the rows, the first;
    /// below them (or on the footer), past the last.
    pub(super) fn pointer_track(&self, rows: &RowLayout, local_y: f32, context: &App) -> usize {
        if local_y < RULER_HEIGHT {
            return 0;
        }
        self.track_at(rows, local_y, context)
            .unwrap_or(self.state(context).track_count())
    }

    /// The top and height, local to the timeline, of the row showing track
    /// `track_index`: past the last track, the rows a drop adds, as tall as
    /// new tracks are.
    pub(super) fn track_span(
        &self,
        rows: &RowLayout,
        track_index: usize,
        context: &App,
    ) -> (f32, f32) {
        let top = RULER_HEIGHT + rows.track_top(track_index) - self.state(context).grid.offset.y;
        let height = rows
            .track_row(track_index)
            .map_or(DEFAULT_TRACK_HEIGHT, |row| row.track.height);
        (top, height)
    }

    pub(super) fn clip_rect(&self, clip: &AudioClip, row: &Row, context: &App) -> Rect {
        self.clip_rect_at(clip, self.row_top(row, context), row.track.height, context)
    }

    /// Where `clip` is on a row at `top`, `height` high, local to the
    /// timeline.
    pub(super) fn clip_rect_at(
        &self,
        clip: &AudioClip,
        top: f32,
        height: f32,
        context: &App,
    ) -> Rect {
        let viewport = self.viewport();
        let start_x = self.state(context).grid.beats_to_x(clip.position, viewport);
        let end_x = self
            .state(context)
            .grid
            .beats_to_x(clip.end(self.state(context).bpm()), viewport);
        Rect::new(start_x, top, end_x - start_x, height)
    }

    /// The clip at `(local_x, local_y)`, its row, and which part of it.
    /// A group's lane only shows an overview of its clips: none there.
    pub(super) fn clip_at(
        &self,
        rows: &RowLayout,
        local_x: f32,
        local_y: f32,
        context: &App,
    ) -> Option<(usize, AudioClip, ClipPart)> {
        let row_index = self.row_at(rows, local_y, context)?;
        let row = &rows[row_index];
        if row.is_group() {
            return None;
        }
        row.track.clips.iter().rev().find_map(|clip| {
            let clip = self.state(context).shown_clip(clip);
            let row = self.clip_rect(&clip, row, context);
            if !row.contains(local_x, local_y) {
                return None;
            }
            let handle = handle_width(row);
            let part = if local_x < row.left + handle {
                ClipPart::TrimStart
            } else if local_x >= row.right() - handle {
                ClipPart::TrimEnd
            } else if local_y < row.top + HEADER_HEIGHT {
                ClipPart::Header
            } else {
                ClipPart::Body
            };
            Some((row_index, clip, part))
        })
    }

    pub(super) fn beats_at(&self, local_x: f32, context: &App) -> BeatPos {
        self.state(context)
            .grid
            .x_to_beats(local_x, self.viewport())
    }

    /// The beat at `local_x` on the grid (unless `alt`), not before the start.
    pub(super) fn snapped(&self, local_x: f32, alt: bool, context: &App) -> BeatPos {
        self.state(context)
            .grid
            .snap(self.beats_at(local_x, context), alt)
            .max(BeatPos::ZERO)
    }
}
