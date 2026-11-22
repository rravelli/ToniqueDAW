//! Drags in the track list: a header's volume or pan, a row's height (its
//! bottom edge), a row to a new place (by its header), and the list's
//! width (its left edge, collapsing it when dragged well past its
//! narrowest).

use gpui::{MouseDownEvent, MouseMoveEvent};
use tonique_engine::edit::TrackId;

use crate::{
    app::Tonique,
    core::{
        state::{MIN_EXPANDED_HEIGHT, RowTarget},
        track::TrackKind,
    },
    headers::{COLLAPSE_PAST, MAX_ROW_HEIGHT},
    timeline::{ROW_GAP, RULER_HEIGHT, rows},
    widgets::{Scale, SliderDrag},
};

/// A header dragged this far moves its row.
const MOVE_THRESHOLD: f32 = 4.;
/// Share of a group header's height, from the top, where a drop goes above
/// the group rather than into it.
const ABOVE_GROUP: f32 = 0.3;

/// Where a dragged row would land, local to the track list: a line in the
/// gap at `y`, or a group's header outlined, dropping into it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RowMarker {
    Line { y: f32 },
    Into { top: f32, height: f32 },
}

/// A row in the list, local to it: its track, kind, top and height.
#[derive(Clone, Copy)]
struct ListRow {
    track: TrackId,
    group: bool,
    top: f32,
    height: f32,
}

/// Where a row dropped at `local_y` lands among `rows`, and where to mark
/// it: into a group past the top of its header, else before the row whose
/// upper half it's on, or the one after; last below them all.
fn row_target(rows: &[ListRow], local_y: f32) -> Option<(RowTarget, RowMarker)> {
    let line_above = |row: &ListRow| RowMarker::Line {
        y: row.top - ROW_GAP / 2.,
    };
    let line_below = |row: &ListRow| RowMarker::Line {
        y: row.top + row.height + ROW_GAP / 2.,
    };
    for (index, row) in rows.iter().enumerate() {
        if local_y > row.top + row.height + ROW_GAP {
            continue;
        }
        let share = (local_y - row.top) / row.height.max(1.);
        let target = if row.group && share > ABOVE_GROUP {
            (
                RowTarget::Into(row.track),
                RowMarker::Into {
                    top: row.top,
                    height: row.height,
                },
            )
        } else if row.group || share < 0.5 {
            (RowTarget::Before(row.track), line_above(row))
        } else {
            match rows.get(index + 1) {
                Some(next) => (RowTarget::Before(next.track), line_above(next)),
                None => (RowTarget::End, line_below(row)),
            }
        };
        return Some(target);
    }
    let last = rows.last()?;
    Some((RowTarget::End, line_below(last)))
}

/// What a header's slider sets.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Param {
    Volume,
    Pan,
}

impl Param {
    pub fn scale(self) -> Scale {
        match self {
            Self::Volume => Scale::VOLUME,
            Self::Pan => Scale::Pan,
        }
    }
}

pub enum HeaderDrag {
    Slider {
        track: TrackId,
        param: Param,
        drag: SliderDrag,
    },
    /// A row's bottom edge: its height follows the pointer.
    Height {
        track: TrackId,
        start_y: f32,
        start_height: f32,
    },
    /// A row by its header: moved once dragged a little.
    Row {
        track: TrackId,
        start_y: f32,
        /// Where the pointer was last, as a release doesn't tell.
        last_y: f32,
        moved: bool,
    },
}

impl Tonique {
    pub(super) fn param(&self, id: TrackId, param: Param) -> f32 {
        let track = self.track_row(id);
        match param {
            Param::Volume => track.map_or(1., |track| track.volume),
            Param::Pan => track.map_or(0., |track| track.pan),
        }
    }

    fn set_param(&mut self, id: TrackId, param: Param, value: f32) {
        match param {
            Param::Volume => self.state.set_volume(id, value),
            Param::Pan => self.state.set_pan(id, value),
        }
    }

    /// `from` to `to`, as one undo step.
    fn commit_param(&mut self, id: TrackId, param: Param, from: f32, to: f32) {
        match param {
            Param::Volume => self.state.commit_volume(id, from, to),
            Param::Pan => self.state.commit_pan(id, from, to),
        }
    }

    /// A press on a slider: a drag from there, or a double-click back to
    /// its default.
    pub(super) fn slider_down(&mut self, id: TrackId, param: Param, event: &MouseDownEvent) {
        let value = self.param(id, param);
        let scale = param.scale();
        if event.click_count >= 2 {
            self.headers.drag = None;
            let default = scale.default_value();
            if value != default {
                self.set_param(id, param, default);
                self.commit_param(id, param, value, default);
            }
            return;
        }
        let pointer_x = f32::from(event.position.x);
        self.headers.drag = Some(HeaderDrag::Slider {
            track: id,
            param,
            drag: SliderDrag::new(scale, value, pointer_x, event.modifiers.shift),
        });
    }

    pub(super) fn row_edge_down(&mut self, id: TrackId, height: f32, event: &MouseDownEvent) {
        self.headers.drag = Some(HeaderDrag::Height {
            track: id,
            start_y: f32::from(event.position.y),
            start_height: height,
        });
    }

    pub(super) fn row_down(&mut self, id: TrackId, event: &MouseDownEvent) {
        let pointer_y = f32::from(event.position.y);
        self.headers.drag = Some(HeaderDrag::Row {
            track: id,
            start_y: pointer_y,
            last_y: pointer_y,
            moved: false,
        });
    }

    /// The row being moved, once dragged a little.
    pub(super) fn moving_row(&self) -> Option<TrackId> {
        match self.headers.drag {
            Some(HeaderDrag::Row {
                track, moved: true, ..
            }) => Some(track),
            _ => None,
        }
    }

    /// Where the row being moved would land, if it's moved and somewhere
    /// it can go: not on itself or into a group it holds.
    pub(super) fn row_drop(&self) -> Option<(TrackId, RowTarget, RowMarker)> {
        let Some(HeaderDrag::Row {
            track,
            last_y,
            moved: true,
            ..
        }) = self.headers.drag
        else {
            return None;
        };
        let offset = self.state.grid.offset.y;
        let rows: Vec<ListRow> = rows(&self.state)
            .iter()
            .map(|row| ListRow {
                track: row.track.id,
                group: row.track.kind == TrackKind::Group,
                top: RULER_HEIGHT + row.top - offset,
                height: row.track.height,
            })
            .collect();
        let local_y = last_y - f32::from(self.headers_bounds().origin.y);
        let (target, marker) = row_target(&rows, local_y)?;
        let allowed = match target {
            RowTarget::Before(row) | RowTarget::Into(row) => {
                row != track && !self.state.ancestors(row).contains(&track)
            }
            RowTarget::End => true,
        };
        allowed.then_some((track, target, marker))
    }

    /// Follows a drag in the track list, `window_width` wide. Returns
    /// whether it's one.
    pub fn header_drag_move(&mut self, event: &MouseMoveEvent, window_width: f32) -> bool {
        let pointer_x = f32::from(event.position.x);
        // The timeline keeps at least as much room as the list.
        if let Some(wanted) = self.headers.width.drag_to(pointer_x, window_width / 2.) {
            if wanted < self.headers.width.min() - COLLAPSE_PAST {
                // Collapsed, at the width it had: expanding finds it again.
                let start = self.headers.width.start_size();
                self.headers.width.end();
                self.headers.width.size = start.unwrap_or(self.headers.width.size);
                self.headers.open = false;
            }
            return true;
        }
        match self.headers.drag.as_mut() {
            None => false,
            Some(HeaderDrag::Slider { track, param, .. }) => {
                let (track, param) = (*track, *param);
                let Some(bounds) = self.headers.sliders.borrow().get(&(track, param)).copied()
                else {
                    return false;
                };
                let current = self.param(track, param);
                let width = f32::from(bounds.size.width);
                let Some(HeaderDrag::Slider { drag, .. }) = self.headers.drag.as_mut() else {
                    return false;
                };
                if let Some(value) = drag.follow(pointer_x, event.modifiers.shift, width, current) {
                    self.set_param(track, param, value);
                }
                true
            }
            Some(HeaderDrag::Height {
                track,
                start_y,
                start_height,
            }) => {
                let moved_by = f32::from(event.position.y) - *start_y;
                let height = (*start_height + moved_by).clamp(MIN_EXPANDED_HEIGHT, MAX_ROW_HEIGHT);
                let track = *track;
                self.state.track_view_mut(&track).height = height;
                true
            }
            Some(HeaderDrag::Row {
                start_y,
                last_y,
                moved,
                ..
            }) => {
                *last_y = f32::from(event.position.y);
                *moved |= (*last_y - *start_y).abs() > MOVE_THRESHOLD;
                true
            }
        }
    }

    pub fn header_drag_end(&mut self) {
        self.headers.width.end();
        let target = self.row_drop();
        match self.headers.drag.take() {
            Some(HeaderDrag::Slider { track, param, drag }) => {
                let value = self.param(track, param);
                if drag.moved && value != drag.start {
                    self.commit_param(track, param, drag.start, value);
                }
            }
            Some(HeaderDrag::Row { .. }) => {
                if let Some((track, target, _)) = target {
                    self.state.move_row(track, target);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: u64, group: bool, top: f32) -> ListRow {
        ListRow {
            track: TrackId(id),
            group,
            top,
            height: 40.,
        }
    }

    #[test]
    fn rows_drop_by_the_middles_of_the_rows() {
        let rows = [row(1, false, 0.), row(2, false, 43.)];
        let before = |y| row_target(&rows, y).map(|(target, _)| target);
        assert_eq!(before(10.), Some(RowTarget::Before(TrackId(1))));
        assert_eq!(before(30.), Some(RowTarget::Before(TrackId(2))));
        assert_eq!(before(70.), Some(RowTarget::End));
        assert_eq!(before(500.), Some(RowTarget::End));
    }

    #[test]
    fn rows_drop_into_groups_below_their_tops() {
        let rows = [row(1, true, 0.), row(2, false, 43.)];
        let target = |y| row_target(&rows, y).map(|(target, _)| target);
        assert_eq!(target(5.), Some(RowTarget::Before(TrackId(1))));
        assert_eq!(target(30.), Some(RowTarget::Into(TrackId(1))));
    }
}
