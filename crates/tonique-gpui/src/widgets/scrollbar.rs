//! A thin scrollbar along one edge: a track, and a thumb to drag, as the
//! egui app's `ScrollBar`. Pressing the track off the thumb jumps the
//! thumb's middle there, then drags it. The thumb lights while the bar
//! is hovered or dragged.
//!
//! The view owning what scrolls keeps a [`ScrollbarGrab`], so a drag lasts
//! across frames, and hands [`scrollbar`] how far it's scrolled and a way
//! to scroll it.

use std::{cell::Cell, rc::Rc};

use gpui::{
    App, Canvas, Context, CursorStyle, DispatchPhase, HitboxBehavior, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ScrollHandle, Window, canvas, point, prelude::*, px,
};

use crate::{
    geometry::Rect,
    theme::{Theme, hsla},
};

/// Thickness of a scrollbar.
pub const SCROLLBAR_WIDTH: f32 = 5.;
/// The thumb never gets shorter than this, to stay grabbable.
const MIN_THUMB: f32 = 16.;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// How far something is scrolled: `offset` between 0 and `max`, with
/// `view` of it shown, all in pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ScrollSpan {
    pub offset: f32,
    pub max: f32,
    pub view: f32,
}

impl ScrollSpan {
    /// The thumb along a track `length` long: where it starts and how long
    /// it is. `None` when nothing scrolls.
    fn thumb(&self, length: f32) -> Option<(f32, f32)> {
        if self.max <= 0. || length <= 0. {
            return None;
        }
        let share = self.view / (self.max + self.view);
        let size = (share * length).max(MIN_THUMB).min(length);
        let start = (self.offset / self.max).clamp(0., 1.) * (length - size);
        Some((start, size))
    }

    /// Pixels of content per pixel the thumb travels.
    fn ratio(&self, length: f32) -> f32 {
        let size = self.thumb(length).map_or(length, |(_, size)| size);
        self.max / (length - size).max(1.)
    }
}

/// While the thumb is dragged: where it was grabbed along the bar, and
/// the offset then.
#[derive(Clone, Default)]
pub struct ScrollbarGrab(Rc<Cell<Option<(f32, f32)>>>);

/// Sets the offset, between 0 and the span's `max`.
type Scroll = Rc<dyn Fn(f32, &mut Window, &mut App)>;

/// What a frame of the bar needs from its prepaint.
pub struct ScrollbarFrame {
    track: Rect,
    span: Option<ScrollSpan>,
    hitbox: Option<gpui::Hitbox>,
}

/// A scrollbar filling the element it's given the bounds of. `span` is
/// asked at prepaint, after what scrolls has been laid out; nothing is
/// drawn when nothing scrolls. `scroll` gets the new offset.
pub fn scrollbar(
    axis: Axis,
    grab: &ScrollbarGrab,
    theme: &Theme,
    span: impl FnOnce(&App) -> Option<ScrollSpan> + 'static,
    scroll: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> Canvas<ScrollbarFrame> {
    let grab = grab.clone();
    let (track_color, thumb_color) = (theme.bg_deep, theme.text_disabled);
    // Hovered or dragged.
    let lit_color = theme.text_muted;
    let scroll: Scroll = Rc::new(scroll);
    canvas(
        move |bounds, window, app| {
            let track = Rect::of(bounds);
            let span = span(app).filter(|span| span.max > 0.);
            let hitbox =
                span.map(|_| window.insert_hitbox(bounds, HitboxBehavior::BlockMouseExceptScroll));
            ScrollbarFrame {
                track,
                span,
                hitbox,
            }
        },
        move |_, frame, window, _| {
            let (Some(span), Some(hitbox)) = (frame.span, frame.hitbox) else {
                grab.0.set(None);
                return;
            };
            let track = frame.track;
            let along = move |x: f32, y: f32| match axis {
                Axis::Horizontal => (x - track.left, track.width),
                Axis::Vertical => (y - track.top, track.height),
            };
            let (_, length) = along(track.left, track.top);
            let Some((start, size)) = span.thumb(length) else {
                return;
            };
            let thumb = match axis {
                Axis::Horizontal => Rect::new(track.left + start, track.top, size, track.height),
                Axis::Vertical => Rect::new(track.left, track.top + start, track.width, size),
            };
            let lit = hitbox.is_hovered(window) || grab.0.get().is_some();
            let color = if lit { lit_color } else { thumb_color };
            window.paint_quad(
                gpui::fill(track.bounds(), hsla(track_color)).corner_radii(gpui::px(2.)),
            );
            window.paint_quad(gpui::fill(thumb.bounds(), hsla(color)).corner_radii(gpui::px(4.)));
            window.set_cursor_style(CursorStyle::Arrow, &hitbox);

            let ratio = span.ratio(length);
            let pressed = grab.clone();
            let on_press = scroll.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, app| {
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || !hitbox.is_hovered(window)
                {
                    return;
                }
                app.stop_propagation();
                let (at, _) = along(f32::from(event.position.x), f32::from(event.position.y));
                let mut offset = span.offset;
                if at < start || at > start + size {
                    // Off the thumb: its middle jumps there.
                    offset = (offset + (at - start - size / 2.) * ratio).clamp(0., span.max);
                    on_press(offset, window, app);
                }
                pressed.0.set(Some((at, offset)));
            });
            let dragged = grab.clone();
            let on_drag = scroll.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, app| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let Some((grabbed_at, grabbed_offset)) = dragged.0.get() else {
                    return;
                };
                if event.pressed_button != Some(MouseButton::Left) {
                    dragged.0.set(None);
                    return;
                }
                let (at, _) = along(f32::from(event.position.x), f32::from(event.position.y));
                let offset = (grabbed_offset + (at - grabbed_at) * ratio).clamp(0., span.max);
                on_drag(offset, window, app);
            });
            let released = grab.clone();
            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, _| {
                if phase == DispatchPhase::Capture {
                    released.0.set(None);
                }
            });
        },
    )
}

/// A scrollbar down the right edge of what `handle` scrolls (an element
/// its parent, positioned, has the size of), `inset` from it: gpui's own
/// scrolling, which shows none.
pub fn handle_scrollbar<V: 'static>(
    handle: &ScrollHandle,
    grab: &ScrollbarGrab,
    inset: f32,
    theme: &Theme,
    context: &Context<V>,
) -> impl IntoElement {
    let (read, write) = (handle.clone(), handle.clone());
    let this = context.weak_entity();
    scrollbar(
        Axis::Vertical,
        grab,
        theme,
        move |_| {
            Some(ScrollSpan {
                offset: -f32::from(read.offset().y),
                max: f32::from(read.max_offset().y),
                view: f32::from(read.bounds().size.height),
            })
        },
        move |offset, _, app| {
            let x = write.offset().x;
            write.set_offset(point(x, px(-offset)));
            // What's drawn from the offset, as pinned rows, follows.
            let _ = this.update(app, |_, context| context.notify());
        },
    )
    .absolute()
    .top_0()
    .bottom_0()
    .right(px(inset))
    .w(px(SCROLLBAR_WIDTH))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thumb_travels_the_track_and_stays_grabbable() {
        let at = |offset| {
            ScrollSpan {
                offset,
                max: 200.,
                view: 200.,
            }
            .thumb(200.)
            .unwrap()
        };
        assert_eq!(at(0.), (0., 100.));
        assert_eq!(at(200.), (100., 100.));
        assert_eq!(at(100.), (50., 100.));

        // Mostly off screen: the thumb keeps its minimum size.
        let long = ScrollSpan {
            offset: 0.,
            max: 99_000.,
            view: 1000.,
        };
        assert_eq!(long.thumb(100.), Some((0., MIN_THUMB)));
        // Dragging the thumb across its travel scrolls all the way.
        assert_eq!(long.ratio(100.) * (100. - MIN_THUMB), 99_000.);

        let fits = ScrollSpan {
            offset: 0.,
            max: 0.,
            view: 100.,
        };
        assert_eq!(fits.thumb(100.), None);
    }
}
