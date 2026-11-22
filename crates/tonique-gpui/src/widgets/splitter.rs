//! A panel's resizable edge: a grip along it that lights in the accent
//! when hovered or dragged, and the panel's width (or height) it sets.

use gpui::{Context, CursorStyle, MouseButton, MouseDownEvent, Stateful, div, prelude::*, px};

use crate::{
    app::Tonique,
    theme::{Theme, hsla},
};

/// The grip's width, inside the panel's edge.
pub const HANDLE_WIDTH: f32 = 5.;

/// The side of its panel the grip is on.
#[derive(Clone, Copy, PartialEq)]
pub enum Edge {
    /// A panel on the right of what it resizes against.
    Left,
    /// A panel on the left.
    Right,
    /// A panel below what it resizes against: its height.
    Top,
}

/// A panel's width, between `min` and `max`, and the drag resizing it.
pub struct Splitter {
    pub size: f32,
    min: f32,
    max: f32,
    edge: Edge,
    /// The pointer's coordinate across the edge and the size when the drag
    /// started.
    drag: Option<(f32, f32)>,
}

impl Splitter {
    pub fn new(edge: Edge, size: f32, min: f32, max: f32) -> Self {
        Self {
            size,
            min,
            max,
            edge,
            drag: None,
        }
    }

    pub fn min(&self) -> f32 {
        self.min
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The size the drag started from, while dragging.
    pub fn start_size(&self) -> Option<f32> {
        self.drag.map(|(_, size)| size)
    }

    fn start(&mut self, pointer: f32) {
        self.drag = Some((pointer, self.size));
    }

    /// Follows the pointer at `pointer` (its x, or its y for a top edge), up
    /// to `limit` at most (and `min` at least). Returns the size the pointer
    /// asks for, unclamped, so the panel can collapse past its narrowest;
    /// `None` when not dragging.
    pub fn drag_to(&mut self, pointer: f32, limit: f32) -> Option<f32> {
        let (from, size) = self.drag?;
        let wanted = match self.edge {
            Edge::Left | Edge::Top => size - (pointer - from),
            Edge::Right => size + (pointer - from),
        };
        self.size = wanted.clamp(self.min, self.max.min(limit).max(self.min));
        Some(wanted)
    }

    pub fn end(&mut self) {
        self.drag = None;
    }
}

/// The grip of the splitter `get` finds, along its panel's edge: the panel
/// must be `relative`.
pub fn splitter_handle(
    id: &'static str,
    splitter: &Splitter,
    theme: &Theme,
    context: &mut Context<Tonique>,
    get: fn(&mut Tonique) -> &mut Splitter,
) -> Stateful<gpui::Div> {
    let accent = hsla(theme.accent);
    let edge = splitter.edge;
    let dragging = splitter.is_dragging();
    let (grip, line) = match edge {
        Edge::Top => (
            div().left_0().right_0().top_0().h(px(HANDLE_WIDTH)),
            div().left_0().right_0().top_0().h(px(2.)),
        ),
        Edge::Left => (
            div().top_0().bottom_0().left_0().w(px(HANDLE_WIDTH)),
            div().top_0().bottom_0().left_0().w(px(2.)),
        ),
        Edge::Right => (
            div().top_0().bottom_0().right_0().w(px(HANDLE_WIDTH)),
            div().top_0().bottom_0().right_0().w(px(2.)),
        ),
    };
    let cursor = match edge {
        Edge::Top => CursorStyle::ResizeUpDown,
        Edge::Left | Edge::Right => CursorStyle::ResizeLeftRight,
    };
    grip.id(id)
        .absolute()
        .cursor(cursor)
        .group(id)
        .child(
            line.absolute()
                .bg(accent)
                .invisible()
                .when(dragging, |element| element.visible())
                .group_hover(id, |style| style.visible()),
        )
        .on_mouse_down(
            MouseButton::Left,
            context.listener(move |this, event: &MouseDownEvent, _, context| {
                context.stop_propagation();
                let pointer = match edge {
                    Edge::Top => event.position.y,
                    Edge::Left | Edge::Right => event.position.x,
                };
                get(this).start(f32::from(pointer));
                context.notify();
            }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_left_grip_widens_its_panel_leftwards() {
        let mut splitter = Splitter::new(Edge::Left, 200., 100., 400.);
        splitter.start(500.);
        assert_eq!(splitter.drag_to(450., 1000.), Some(250.));
        assert_eq!(splitter.size, 250.);
        // Past the narrowest: clamped, but the wish is told.
        assert_eq!(splitter.drag_to(700., 1000.), Some(0.));
        assert_eq!(splitter.size, 100.);
        // Never wider than the room it's given.
        splitter.drag_to(0., 300.);
        assert_eq!(splitter.size, 300.);
    }

    #[test]
    fn a_right_grip_widens_its_panel_rightwards() {
        let mut splitter = Splitter::new(Edge::Right, 200., 100., 400.);
        splitter.start(200.);
        assert_eq!(splitter.drag_to(260., 1000.), Some(260.));
        splitter.end();
        assert_eq!(splitter.drag_to(300., 1000.), None);
    }

    #[test]
    fn a_top_grip_heightens_its_panel_upwards() {
        let mut splitter = Splitter::new(Edge::Top, 220., 220., 400.);
        splitter.start(600.);
        assert_eq!(splitter.drag_to(550., 1000.), Some(270.));
        assert_eq!(splitter.size, 270.);
    }
}
