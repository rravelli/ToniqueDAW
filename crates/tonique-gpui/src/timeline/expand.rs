//! Collapsed groups opened while clips or audio are dragged over them, as
//! the egui timeline's: what's inside shows, to drop onto, and closes
//! again once the pointer leaves the group. Dropping inside leaves it
//! open, to show where things landed.

use crate::model::HoldsProject;
use crate::timeline::Arrangement;
use gpui::App;
use tonique_engine::edit::TrackId;

use crate::timeline::{ROW_GAP, RULER_HEIGHT};

impl Arrangement {
    /// Follows a drag with the pointer at `local_y` over the lanes (`None`
    /// when off them); `dragging` is false once it's dropped or let go.
    pub(super) fn expand_on_hover(
        &mut self,
        local_y: Option<f32>,
        dragging: bool,
        context: &mut App,
    ) {
        let local_y = local_y.filter(|&y| y >= RULER_HEIGHT && !self.in_footer(y, context));
        if dragging && let Some(pointer) = local_y {
            for row in self.rows(context).iter() {
                let top = self.row_top(row, context);
                let track = &row.track;
                if row.is_group()
                    && track.collapsed
                    && pointer >= top
                    && pointer < top + track.height + ROW_GAP
                {
                    self.state_mut(context).set_collapsed(&track.id, false);
                    self.timeline.hover_expanded.push(track.id);
                }
            }
        }
        let rows = self.rows(context);
        // A group's span runs to the end of its last row.
        let inside = |group: TrackId| {
            rows.iter()
                .find(|row| row.track.id == group)
                .zip(local_y)
                .is_some_and(|(row, pointer)| {
                    let top = self.row_top(row, context);
                    let bottom = top + row.scope_bottom - row.top + ROW_GAP;
                    pointer >= top && pointer < bottom
                })
        };
        let (kept, left): (Vec<TrackId>, Vec<TrackId>) = self
            .timeline
            .hover_expanded
            .iter()
            .partition(|group| inside(**group));
        for group in &left {
            self.state_mut(context).set_collapsed(group, true);
        }
        // Still over it, it stays open for now; dropped inside, for good.
        self.timeline.hover_expanded = if dragging { kept } else { Vec::new() };
    }
}
