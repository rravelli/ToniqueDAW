//! A thin scrollbar along one edge: a track, and a thumb to drag.

use crate::ui::theme::ThemeExt;
use egui::{Id, Rect, Response, Sense, Ui, Widget, pos2};

/// Thickness of a scrollbar.
pub const SCROLLBAR_WIDTH: f32 = 5.;
/// The thumb never gets shorter than this, to stay grabbable.
const MIN_THUMB: f32 = 16.;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// Scrolls `offset` between 0 and `max_scroll` by dragging its thumb. The
/// response is dragged while it scrolls.
pub struct ScrollBar<'a> {
    id: Id,
    axis: Axis,
    /// Where the bar goes.
    track: Rect,
    offset: &'a mut f32,
    max_scroll: f32,
    /// Share of the content in view, which sizes the thumb.
    visible: f32,
}

impl<'a> ScrollBar<'a> {
    pub fn new(
        id: impl Into<Id>,
        axis: Axis,
        track: Rect,
        offset: &'a mut f32,
        max_scroll: f32,
        visible: f32,
    ) -> Self {
        Self {
            id: id.into(),
            axis,
            track,
            offset,
            max_scroll,
            visible,
        }
    }
}

/// Length of the track along the axis.
fn length(axis: Axis, track: Rect) -> f32 {
    match axis {
        Axis::Horizontal => track.width(),
        Axis::Vertical => track.height(),
    }
}

/// Where the thumb is, for `offset` out of `max_scroll`.
fn thumb(axis: Axis, track: Rect, offset: f32, max_scroll: f32, visible: f32) -> Rect {
    let size = (visible * length(axis, track)).max(MIN_THUMB);
    let start = (offset / max_scroll) * (length(axis, track) - size);
    match axis {
        Axis::Horizontal => {
            let x = track.left() + start;
            Rect::from_min_max(pos2(x, track.top()), pos2(x + size, track.bottom()))
        }
        Axis::Vertical => {
            let y = track.top() + start;
            Rect::from_min_max(pos2(track.left(), y), pos2(track.right(), y + size))
        }
    }
}

impl Widget for ScrollBar<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let thumb = thumb(
            self.axis,
            self.track,
            *self.offset,
            self.max_scroll,
            self.visible,
        );
        let response = ui.interact(thumb, ui.id().with(self.id), Sense::click_and_drag());
        let theme = ui.app_theme();
        let painter = ui.painter();
        painter.rect_filled(self.track, 2.0, theme.bg_deep);
        painter.rect_filled(thumb, 4.0, theme.text_disabled);

        if response.dragged() {
            let (drag, thumb_size) = match self.axis {
                Axis::Horizontal => (response.drag_delta().x, thumb.width()),
                Axis::Vertical => (response.drag_delta().y, thumb.height()),
            };
            // Points of content per point of thumb travel.
            let ratio = self.max_scroll / (length(self.axis, self.track) - thumb_size);
            *self.offset = (*self.offset + drag * ratio).clamp(0., self.max_scroll);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn the_thumb_travels_the_track_and_stays_grabbable() {
        let track = Rect::from_min_size(pos2(10., 0.), vec2(200., 5.));
        let at = |offset| thumb(Axis::Horizontal, track, offset, 400., 0.5);
        assert_eq!(at(0.).left(), 10.);
        assert_eq!(at(0.).width(), 100.);
        assert_eq!(at(400.).right(), 210.);
        assert_eq!(at(200.).left(), 60.);

        // Mostly off screen: the thumb keeps its minimum size.
        let tall = Rect::from_min_size(pos2(0., 0.), vec2(5., 100.));
        let small = thumb(Axis::Vertical, tall, 0., 1000., 0.01);
        assert_eq!(small.height(), MIN_THUMB);
        assert_eq!(small.width(), 5.);
    }
}
