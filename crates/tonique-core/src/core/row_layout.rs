//! Where each row of the track list is, down the arrangement: shared by
//! the track headers and the timeline of every UI, so they agree. Built
//! from the project ([`ProjectState::row_layout`]) each time it changes,
//! not on every look.

use crate::core::{
    state::ProjectState,
    track::{DEFAULT_TRACK_HEIGHT, TrackKind, TrackRow},
};

/// Between rows.
pub const ROW_GAP: f32 = 3.;

/// A row of the track list: a track, or a group's header.
#[derive(Debug, Clone)]
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

    /// Without the gap below.
    pub fn bottom(&self) -> f32 {
        self.top + self.track.height
    }

    /// Whether the row, its gap below included, overlaps `(top, bottom)`.
    pub fn overlaps(&self, (top, bottom): (f32, f32)) -> bool {
        self.top < bottom && self.bottom() + ROW_GAP > top
    }
}

/// The rows shown, tracks and groups, without what's inside collapsed
/// groups, from `origin` down.
#[derive(Debug, Clone, Default)]
pub struct RowLayout {
    rows: Vec<Row>,
    /// For each track (in the engine's order), the row showing it: its own,
    /// or its collapsed group's.
    track_rows: Vec<usize>,
    origin: f32,
    /// Below the last row's gap.
    end: f32,
}

impl RowLayout {
    pub fn new(state: &ProjectState, origin: f32) -> Self {
        let mut top = origin;
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
                .map(Row::bottom);
            if let Some(bottom) = last_inside {
                rows[index].scope_bottom = bottom;
            }
        }
        // Rows are in track order: a track is shown by the last row
        // starting at or before it (its own, or the collapsed group
        // hiding it).
        let mut track_rows = Vec::with_capacity(state.track_count());
        let mut row = 0;
        for track in 0..state.track_count() {
            while row + 1 < rows.len() && rows[row + 1].track.first_track_index <= track {
                row += 1;
            }
            track_rows.push(row);
        }
        Self {
            rows,
            track_rows,
            origin,
            end: top,
        }
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn origin(&self) -> f32 {
        self.origin
    }

    /// Height of every row, gaps included.
    pub fn height(&self) -> f32 {
        self.end - self.origin
    }

    /// The index of the row showing track `track_index` (in the engine's
    /// order): its own, or its collapsed group's.
    pub fn showing_row(&self, track_index: usize) -> Option<usize> {
        self.track_rows.get(track_index).copied()
    }

    /// The row showing track `track_index`.
    pub fn track_row(&self, track_index: usize) -> Option<&Row> {
        self.showing_row(track_index).map(|index| &self.rows[index])
    }

    /// Top of the row showing track `track_index`. Past the last track,
    /// where new tracks would go.
    pub fn track_top(&self, track_index: usize) -> f32 {
        match self.track_row(track_index) {
            Some(row) => row.top,
            None => {
                let past = track_index - self.track_rows.len();
                self.end + past as f32 * (DEFAULT_TRACK_HEIGHT + ROW_GAP)
            }
        }
    }

    /// The index of the row at `y`, the gap below a row counting as the
    /// row.
    pub fn row_at(&self, y: f32) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| y >= row.top && y < row.bottom() + ROW_GAP)
    }

    /// The track at `y`, in the engine's order: on a group's header, its
    /// first (clips always land on tracks).
    pub fn track_at(&self, y: f32) -> Option<usize> {
        self.row_at(y)
            .map(|row| self.rows[row].track.first_track_index)
    }
}

/// The rows, top to bottom.
impl std::ops::Deref for RowLayout {
    type Target = [Row];

    fn deref(&self) -> &[Row] {
        &self.rows
    }
}

impl ProjectState {
    /// The rows of the track list laid out from 0 down.
    pub fn row_layout(&self) -> RowLayout {
        RowLayout::new(self, 0.)
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
        let group = state.group(&tracks[1..3]).expect("tracks to group");
        state.set_collapsed(&group, collapsed);
        state
    }

    #[test]
    fn groups_are_rows_scoping_down_to_their_last_track() {
        let layout = RowLayout::new(&state_with_group(false), 70.);
        let rows = layout.rows();
        assert_eq!(rows.len(), 5, "t0, group, t1, t2, t3");
        assert_eq!(rows[0].top, 70.);
        for pair in rows.windows(2) {
            assert_eq!(pair[1].top, pair[0].bottom() + ROW_GAP);
        }
        assert!(rows[1].is_group());
        assert_eq!(rows[2].track.depth, 1);
        assert_eq!(rows[1].scope_bottom, rows[3].bottom());
        assert_eq!(rows[0].scope_bottom, rows[0].bottom());
        assert_eq!(layout.height(), rows[4].bottom() + ROW_GAP - 70.);
    }

    #[test]
    fn rows_overlap_a_span_with_their_gap_below() {
        let layout = state_with_group(false).row_layout();
        let second = &layout.rows()[1];
        let bottom = second.bottom() + ROW_GAP;
        assert!(second.overlaps((second.top, second.top + 1.)));
        assert!(second.overlaps((bottom - 1., bottom + 100.)));
        assert!(!second.overlaps((bottom, bottom + 100.)));
        assert!(!second.overlaps((0., second.top)));
    }

    #[test]
    fn tracks_show_on_their_rows_or_their_collapsed_groups() {
        let open = state_with_group(false).row_layout();
        let shown: Vec<_> = (0..4).map(|track| open.showing_row(track)).collect();
        assert_eq!(shown, [Some(0), Some(2), Some(3), Some(4)]);
        let collapsed = state_with_group(true).row_layout();
        assert_eq!(collapsed.rows().len(), 3, "t0, group, t3");
        let shown: Vec<_> = (0..4).map(|track| collapsed.showing_row(track)).collect();
        assert_eq!(shown, [Some(0), Some(1), Some(1), Some(2)]);
        assert_eq!(collapsed.track_top(2), collapsed.rows()[1].top);
    }

    #[test]
    fn past_the_last_track_new_rows_follow() {
        let layout = state_with_group(true).row_layout();
        assert_eq!(layout.track_top(4), layout.height());
        assert_eq!(
            layout.track_top(5),
            layout.height() + DEFAULT_TRACK_HEIGHT + ROW_GAP
        );
    }

    #[test]
    fn the_track_at_a_group_header_is_its_first() {
        let layout = state_with_group(false).row_layout();
        let rows = layout.rows();
        let middle = |row: &Row| (row.top + row.bottom()) / 2.;
        assert_eq!(layout.track_at(middle(&rows[0])), Some(0));
        assert_eq!(
            layout.track_at(middle(&rows[1])),
            Some(1),
            "the group's first"
        );
        assert_eq!(
            layout.track_at(rows[1].bottom() + 1.),
            Some(1),
            "in its gap"
        );
        assert_eq!(layout.track_at(layout.height() + 1.), None);
    }
}
