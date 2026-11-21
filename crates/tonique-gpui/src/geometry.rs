//! Rectangles in window coordinates, for what's painted on canvases.

use gpui::{Bounds, Pixels, point, px, size};

/// A rectangle in window coordinates (logical pixels).
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self {
            left,
            top,
            width,
            height,
        }
    }
    pub fn right(&self) -> f32 {
        self.left + self.width
    }
    pub fn bottom(&self) -> f32 {
        self.top + self.height
    }
    pub fn intersect(&self, other: Rect) -> Rect {
        let left = self.left.max(other.left);
        let top = self.top.max(other.top);
        Rect::new(
            left,
            top,
            (self.right().min(other.right()) - left).max(0.),
            (self.bottom().min(other.bottom()) - top).max(0.),
        )
    }
    pub fn is_empty(&self) -> bool {
        self.width <= 0. || self.height <= 0.
    }
    pub fn contains(&self, point_x: f32, point_y: f32) -> bool {
        point_x >= self.left
            && point_x < self.right()
            && point_y >= self.top
            && point_y < self.bottom()
    }
    pub fn of(bounds: Bounds<Pixels>) -> Self {
        Self::new(
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        )
    }
    pub fn bounds(&self) -> Bounds<Pixels> {
        Bounds::new(
            point(px(self.left), px(self.top)),
            size(px(self.width), px(self.height)),
        )
    }
}
