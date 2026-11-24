//! Where each row of the track list is on screen: the core's layout
//! (`tonique_core::session::row_layout`) placed in the viewport, once a frame, and
//! shared by the track headers and the timeline, so they agree.

use egui::Rangef;

pub use tonique_core::session::row_layout::Row;
use tonique_core::{project::ProjectState, session::row_layout};

pub struct RowLayout {
    /// From the top of the viewport, scrolled.
    layout: row_layout::RowLayout,
    /// Top of the viewport.
    view_top: f32,
    scroll: f32,
}

impl RowLayout {
    /// Rows from the top of a viewport scrolled down by `scroll`.
    pub fn new(state: &ProjectState, view_top: f32, scroll: f32) -> Self {
        Self {
            layout: row_layout::RowLayout::new(state, view_top - scroll),
            view_top,
            scroll,
        }
    }

    /// Lay out again, after rows changed (collapsed, expanded or added).
    pub fn rebuild(&mut self, state: &ProjectState) {
        *self = Self::new(state, self.view_top, self.scroll);
    }

    pub fn rows(&self) -> &[Row] {
        self.layout.rows()
    }

    /// Height of every row, gaps included.
    pub fn height(&self) -> f32 {
        self.layout.height()
    }

    /// The row showing track `index`: its own, or its collapsed group's.
    pub fn track_row(&self, index: usize) -> Option<&Row> {
        self.layout.track_row(index)
    }

    /// Top of the row showing track `index`. Past the last track, where new
    /// tracks would go.
    pub fn track_top(&self, index: usize) -> f32 {
        self.layout.track_top(index)
    }

    /// The track under screen `y` (the gap below a row counts as the row),
    /// and the top of the row showing it. Over a group's header, its first
    /// track: clips always land on tracks. Above the viewport, the first
    /// track; below the last row, `None`.
    pub fn track_at(&self, y: f32) -> Option<(usize, f32)> {
        if self.layout.is_empty() {
            return None;
        }
        let index = if y <= self.view_top {
            0
        } else {
            self.layout.track_at(y)?
        };
        Some((index, self.track_top(index)))
    }
}

/// A row's spans on screen.
pub trait RowSpans {
    /// The row, without the gap below.
    fn y(&self) -> Rangef;
    /// For an expanded group, down to the end of its last row; else `y`.
    fn scope(&self) -> Rangef;
}

impl RowSpans for Row {
    fn y(&self) -> Rangef {
        Rangef::new(self.top, self.bottom())
    }

    fn scope(&self) -> Rangef {
        Rangef::new(self.top, self.scope_bottom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonique_core::{document::track::DEFAULT_TRACK_HEIGHT, session::row_layout::ROW_GAP};
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
        assert_eq!(rows[0].y().min, 70.);
        for pair in rows.windows(2) {
            assert_eq!(pair[1].y().min, pair[0].y().max + ROW_GAP);
        }
        assert_eq!(layout.height(), rows[4].y().max + ROW_GAP - 70.);
        // The group's scope runs to the bottom of t2.
        assert_eq!(
            rows[1].scope(),
            Rangef::new(rows[1].y().min, rows[3].y().max)
        );
        assert_eq!(rows[0].scope(), rows[0].y());
    }

    #[test]
    fn tracks_in_a_collapsed_group_are_at_its_row() {
        let state = state_with_group(true);
        let layout = RowLayout::new(&state, 0., 0.);
        let rows = layout.rows();
        assert_eq!(rows.len(), 3, "t0, group, t3");
        assert_eq!(layout.track_top(1), rows[1].y().min);
        assert_eq!(layout.track_top(2), rows[1].y().min);
        assert_eq!(layout.track_top(3), rows[2].y().min);
        // Past the end: where new tracks go.
        assert_eq!(
            layout.track_top(5),
            layout.height() + DEFAULT_TRACK_HEIGHT + ROW_GAP
        );
    }

    #[test]
    fn track_at_finds_tracks_not_groups() {
        let state = state_with_group(false);
        let layout = RowLayout::new(&state, 0., 0.);
        let rows = layout.rows();
        let middle = |r: &Row| (r.y().min + r.y().max) / 2.;
        assert_eq!(layout.track_at(-5.), Some((0, rows[0].y().min)));
        assert_eq!(
            layout.track_at(middle(&rows[0])),
            Some((0, rows[0].y().min))
        );
        // The group's header: its first track, t1.
        assert_eq!(
            layout.track_at(middle(&rows[1])),
            Some((1, rows[2].y().min))
        );
        assert_eq!(
            layout.track_at(middle(&rows[3])),
            Some((2, rows[3].y().min))
        );
        // The gap below a row is part of it.
        assert_eq!(
            layout.track_at(rows[3].y().max + ROW_GAP / 2.),
            Some((2, rows[3].y().min))
        );
        assert_eq!(layout.track_at(layout.height() + 50.), None);
    }
}
