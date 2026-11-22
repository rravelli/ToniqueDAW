//! Where things are on the timeline: its rows, the clips on them, and the
//! beats under the pointer.

use crate::model::HoldsProject;
use gpui::{App, Pixels, Point, Window};
use tonique_engine::time::BeatPos;

use crate::{
    app::Tonique,
    core::{
        clip::AudioClip,
        grid::Viewport,
        state::ProjectState,
        track::{DEFAULT_TRACK_HEIGHT, TrackKind, TrackRow},
    },
    geometry::Rect,
    timeline::{ROW_GAP, RULER_HEIGHT},
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

/// A row of the track list (a track, or a group's header): its top in
/// the scrolled content, and height.
pub struct Row {
    pub track: TrackRow,
    pub top: f32,
    /// For an expanded group, the bottom of its last row inside; else the
    /// row's own bottom.
    pub scope_bottom: f32,
}

impl Row {
    pub fn is_group(&self) -> bool {
        self.track.kind == TrackKind::Group
    }

    /// Whether the row, its gap below included, overlaps `span`: a top and
    /// a bottom in the scrolled content.
    pub fn overlaps(&self, (top, bottom): (f32, f32)) -> bool {
        self.top < bottom && self.top + self.track.height + ROW_GAP > top
    }
}

/// The rows shown, tracks and groups, without what's inside collapsed
/// groups.
pub fn rows(state: &ProjectState) -> Vec<Row> {
    let mut top = 0.;
    let mut rows: Vec<Row> = state
        .rows()
        .into_iter()
        .map(|track| {
            let row = Row {
                top,
                scope_bottom: top + track.height,
                track,
            };
            top += row.track.height + ROW_GAP;
            row
        })
        .collect();
    for index in 0..rows.len() {
        if !rows[index].is_group() {
            continue;
        }
        let depth = rows[index].track.depth;
        let last_inside = rows[index + 1..]
            .iter()
            .take_while(|row| row.track.depth > depth)
            .last()
            .map(|row| row.top + row.track.height);
        if let Some(bottom) = last_inside {
            rows[index].scope_bottom = bottom;
        }
    }
    rows
}

/// The row showing track `track_index` (in the engine's order): its own,
/// or the collapsed group's hiding it. Rows are in track order, so it's
/// the last one starting at or before it.
fn showing_row(rows: &[Row], track_index: usize) -> Option<usize> {
    rows.iter()
        .rposition(|row| row.track.first_track_index <= track_index)
}

/// Above the master's header, below the footer's top line.
pub const FOOTER_PADDING: f32 = 3.;

/// The footer holding the master, below the rows of the timeline and of
/// the track list: as tall as the master's row.
pub fn footer_height(state: &ProjectState) -> f32 {
    state.master_track().height + FOOTER_PADDING
}

/// How tall the rows are, gaps included.
pub fn rows_height(rows: &[Row]) -> f32 {
    rows.last()
        .map_or(0., |row| row.top + row.track.height + ROW_GAP)
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

impl Tonique {
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
    pub(super) fn row_at(&self, rows: &[Row], local_y: f32, context: &App) -> Option<usize> {
        if self.in_footer(local_y, context) {
            return None;
        }
        rows.iter().position(|row| {
            let top = self.row_top(row, context);
            local_y >= top && local_y < top + row.track.height + ROW_GAP
        })
    }

    /// The track under `local_y`, in the engine's order: on a group's
    /// header, its first (clips always land on tracks).
    pub(super) fn track_at(&self, rows: &[Row], local_y: f32, context: &App) -> Option<usize> {
        self.row_at(rows, local_y, context)
            .map(|row| rows[row].track.first_track_index)
    }

    /// The track under `local_y`, for a drag: above the rows, the first;
    /// below them (or on the footer), past the last.
    pub(super) fn pointer_track(&self, rows: &[Row], local_y: f32, context: &App) -> usize {
        if local_y < RULER_HEIGHT {
            return 0;
        }
        self.track_at(rows, local_y, context)
            .unwrap_or(self.state(context).track_count())
    }

    /// The top and height, local to the timeline, of the row showing track
    /// `track_index`: past the last track, the rows a drop adds, as tall as
    /// new tracks are.
    pub(super) fn track_span(&self, rows: &[Row], track_index: usize, context: &App) -> (f32, f32) {
        let track_count = self.state(context).track_count();
        if track_index < track_count
            && let Some(row) = showing_row(rows, track_index)
        {
            let row = &rows[row];
            return (self.row_top(row, context), row.track.height);
        }
        let past =
            track_index.saturating_sub(track_count) as f32 * (DEFAULT_TRACK_HEIGHT + ROW_GAP);
        let top = RULER_HEIGHT + rows_height(rows) + past - self.state(context).grid.offset.y;
        (top, DEFAULT_TRACK_HEIGHT)
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
        rows: &[Row],
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

#[cfg(test)]
mod tests {
    use super::*;
    use tonique_engine::engine::{Engine, EngineConfig};

    fn state_with_group(collapsed: bool) -> ProjectState {
        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        let tracks: Vec<_> = (0..4).map(|_| state.add_track()).collect();
        // t0, [group: t1, t2], t3
        let group = state.group(&tracks[1..3]).expect("grouped");
        state.set_collapsed(&group, collapsed);
        state
    }

    #[test]
    fn groups_are_rows_scoping_down_to_their_last_track() {
        let state = state_with_group(false);
        let rows = rows(&state);
        assert_eq!(rows.len(), 5, "t0, group, t1, t2, t3");
        for pair in rows.windows(2) {
            assert_eq!(pair[1].top, pair[0].top + pair[0].track.height + ROW_GAP);
        }
        assert!(rows[1].is_group());
        assert_eq!(rows[2].track.depth, 1);
        assert_eq!(rows[1].scope_bottom, rows[3].top + rows[3].track.height);
        assert_eq!(rows[0].scope_bottom, rows[0].top + rows[0].track.height);
    }

    #[test]
    fn rows_overlap_a_span_with_their_gap_below() {
        let rows = rows(&state_with_group(false));
        let second = &rows[1];
        let bottom = second.top + second.track.height + ROW_GAP;
        assert!(second.overlaps((second.top, second.top + 1.)));
        assert!(second.overlaps((bottom - 1., bottom + 100.)));
        assert!(!second.overlaps((bottom, bottom + 100.)));
        assert!(!second.overlaps((0., second.top)));
        let in_view: Vec<_> = rows.iter().filter(|row| row.overlaps((0., 1.))).collect();
        assert_eq!(in_view.len(), 1, "only the first");
    }

    #[test]
    fn tracks_show_on_their_rows_or_their_collapsed_groups() {
        let open = rows(&state_with_group(false));
        assert_eq!(showing_row(&open, 0), Some(0));
        assert_eq!(showing_row(&open, 1), Some(2));
        assert_eq!(showing_row(&open, 2), Some(3));
        assert_eq!(showing_row(&open, 3), Some(4));

        let collapsed = rows(&state_with_group(true));
        assert_eq!(collapsed.len(), 3, "t0, group, t3");
        assert_eq!(showing_row(&collapsed, 1), Some(1));
        assert_eq!(showing_row(&collapsed, 2), Some(1));
        assert_eq!(showing_row(&collapsed, 3), Some(2));
    }
}
