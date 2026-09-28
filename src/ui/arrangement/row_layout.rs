//! Where each row of the track list is on screen: computed once a frame and
//! shared by the track headers and the timeline, so they agree.

use egui::Rangef;

use crate::{
    core::{
        state::ProjectState,
        track::{DEFAULT_TRACK_HEIGHT, TrackKind, TrackRow},
    },
    ui::arrangement::track_header::ROW_GAP,
};

/// A row of the track list: a track, or a group's header.
pub struct Row {
    pub track: TrackRow,
    /// On screen, without the gap below.
    pub y: Rangef,
    /// For an expanded group, down to the end of its last row; else `y`.
    pub scope: Rangef,
}

pub struct RowLayout {
    rows: Vec<Row>,
    /// For each track (in the engine's order), the row showing it: its own,
    /// or its collapsed group's.
    track_rows: Vec<usize>,
    /// Top of the viewport.
    view_top: f32,
    scroll: f32,
    /// Below the last row's gap.
    end: f32,
}

impl RowLayout {
    /// Rows from the top of a viewport scrolled down by `scroll`.
    pub fn new(state: &ProjectState, view_top: f32, scroll: f32) -> Self {
        let mut y = view_top - scroll;
        let mut rows: Vec<Row> = state
            .rows()
            .into_iter()
            .map(|track| {
                let span = Rangef::new(y, y + track.height);
                y += track.height + ROW_GAP;
                Row {
                    track,
                    y: span,
                    scope: span,
                }
            })
            .collect();
        for i in 0..rows.len() {
            if rows[i].track.kind != TrackKind::Group {
                continue;
            }
            let depth = rows[i].track.depth;
            let last = rows[i + 1..]
                .iter()
                .take_while(|r| r.track.depth > depth)
                .last()
                .map(|r| r.y.max);
            if let Some(bottom) = last {
                rows[i].scope.max = bottom;
            }
        }
        // Rows are in track order: a track is shown by the last row starting
        // at or before it (its own, or the collapsed group hiding it).
        let mut track_rows = Vec::with_capacity(state.track_count());
        let mut r = 0;
        for track in 0..state.track_count() {
            while r + 1 < rows.len() && rows[r + 1].track.index <= track {
                r += 1;
            }
            track_rows.push(r);
        }
        Self {
            rows,
            track_rows,
            view_top,
            scroll,
            end: y,
        }
    }

    /// Lay out again, after rows were collapsed or expanded.
    pub fn rebuild(&mut self, state: &ProjectState) {
        *self = Self::new(state, self.view_top, self.scroll);
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Height of every row, gaps included.
    pub fn height(&self) -> f32 {
        self.end - (self.view_top - self.scroll)
    }

    /// The row showing track `index`: its own, or its collapsed group's.
    pub fn track_row(&self, index: usize) -> Option<&Row> {
        self.track_rows.get(index).map(|r| &self.rows[*r])
    }

    /// Top of the row showing track `index`. Past the last track, where new
    /// tracks would go.
    pub fn track_top(&self, index: usize) -> f32 {
        match self.track_row(index) {
            Some(row) => row.y.min,
            None => {
                let past = index - self.track_rows.len();
                self.end + past as f32 * (DEFAULT_TRACK_HEIGHT + ROW_GAP)
            }
        }
    }

    /// The track under screen `y` (the gap below a row counts as the row),
    /// and the top of the row showing it. Over a group's header, its first
    /// track: clips always land on tracks. Above the viewport, the first
    /// track; below the last row, `None`.
    pub fn track_at(&self, y: f32) -> Option<(usize, f32)> {
        if self.track_rows.is_empty() {
            return None;
        }
        let index = if y <= self.view_top {
            0
        } else {
            self.rows
                .iter()
                .find(|r| r.y.min <= y && y <= r.y.max + ROW_GAP)?
                .track
                .index
        };
        Some((index, self.track_top(index)))
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
        let group = state.group(&tracks[1..3]).unwrap();
        state.set_collapsed(&group, collapsed);
        state
    }

    #[test]
    fn rows_stack_from_the_scrolled_top() {
        let state = state_with_group(false);
        let layout = RowLayout::new(&state, 100., 30.);
        let rows = layout.rows();
        assert_eq!(rows.len(), 5, "t0, group, t1, t2, t3");
        assert_eq!(rows[0].y.min, 70.);
        for pair in rows.windows(2) {
            assert_eq!(pair[1].y.min, pair[0].y.max + ROW_GAP);
        }
        assert_eq!(layout.height(), layout.end - 70.);
        // The group's scope runs to the bottom of t2.
        assert_eq!(rows[1].scope, Rangef::new(rows[1].y.min, rows[3].y.max));
        assert_eq!(rows[0].scope, rows[0].y);
    }

    #[test]
    fn tracks_in_a_collapsed_group_are_at_its_row() {
        let state = state_with_group(true);
        let layout = RowLayout::new(&state, 0., 0.);
        let rows = layout.rows();
        assert_eq!(rows.len(), 3, "t0, group, t3");
        assert_eq!(layout.track_top(1), rows[1].y.min);
        assert_eq!(layout.track_top(2), rows[1].y.min);
        assert_eq!(layout.track_top(3), rows[2].y.min);
        // Past the end: where new tracks go.
        assert_eq!(
            layout.track_top(5),
            layout.end + DEFAULT_TRACK_HEIGHT + ROW_GAP
        );
    }

    #[test]
    fn track_at_finds_tracks_not_groups() {
        let state = state_with_group(false);
        let layout = RowLayout::new(&state, 0., 0.);
        let rows = layout.rows();
        let middle = |r: &Row| (r.y.min + r.y.max) / 2.;
        assert_eq!(layout.track_at(-5.), Some((0, rows[0].y.min)));
        assert_eq!(layout.track_at(middle(&rows[0])), Some((0, rows[0].y.min)));
        // The group's header: its first track, t1.
        assert_eq!(layout.track_at(middle(&rows[1])), Some((1, rows[2].y.min)));
        assert_eq!(layout.track_at(middle(&rows[3])), Some((2, rows[3].y.min)));
        // The gap below a row is part of it.
        assert_eq!(
            layout.track_at(rows[3].y.max + ROW_GAP / 2.),
            Some((2, rows[3].y.min))
        );
        assert_eq!(layout.track_at(layout.end + 50.), None);
    }
}
