//! Wheel scrolling eased over a few frames, as egui does it
//! (`input_state/wheel_state.rs`): a mouse wheel's notch is a large step,
//! so it's applied bit by bit (90 % of it within 0.1 s) rather than in one
//! jump. A trackpad's small steps are applied as they come, as easing them
//! would only add latency.

use std::time::Instant;

use gpui::ScrollDelta;

/// Pixels a wheel line scrolls, as egui's `line_scroll_speed`.
const LINE: f32 = 40.;
/// Pixel steps this large come from a wheel, smaller ones from a trackpad
/// (egui's threshold).
const WHEEL_STEP: f32 = 8.;
/// Eased: this much of a step is applied within `EASE_SECONDS`.
const EASE_SHARE: f32 = 0.9;
const EASE_SECONDS: f32 = 0.1;
/// The frame assumed for the first step after an idle time.
const FIRST_FRAME_SECONDS: f32 = 1. / 60.;
/// A step longer than this (a stalled frame) counts as this long.
const LONGEST_FRAME_SECONDS: f32 = 0.1;
/// Less than this left is applied at once.
const LAST_PIXEL: f32 = 1.;

/// A wheel event's movement in pixels, and whether it should be eased.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelDelta {
    pub horizontal: f32,
    pub vertical: f32,
    pub eased: bool,
}

impl WheelDelta {
    pub fn of(delta: ScrollDelta) -> Self {
        match delta {
            ScrollDelta::Lines(lines) => Self {
                horizontal: lines.x * LINE,
                vertical: lines.y * LINE,
                eased: true,
            },
            ScrollDelta::Pixels(pixels) => {
                let (horizontal, vertical) = (f32::from(pixels.x), f32::from(pixels.y));
                Self {
                    horizontal,
                    vertical,
                    eased: horizontal.hypot(vertical) >= WHEEL_STEP,
                }
            }
        }
    }
}

/// An amount scrolled in one direction (or zoomed), applied frame by
/// frame.
#[derive(Default)]
pub struct SmoothScroll {
    /// What's left to apply.
    pending: f32,
    /// When the last part was applied, while there's some left.
    last_step: Option<Instant>,
}

impl SmoothScroll {
    /// Takes `amount`. Returns what to apply now: all of it when not
    /// `eased`, nothing otherwise (the frames to come apply it).
    pub fn push(&mut self, amount: f32, eased: bool) -> f32 {
        if !eased {
            return amount;
        }
        self.pending += amount;
        0.
    }

    /// Whether some is left to apply: frames must keep coming.
    pub fn is_active(&self) -> bool {
        self.pending != 0.
    }

    /// Drops what's left, as when it can't go further.
    pub fn stop(&mut self) {
        self.pending = 0.;
        self.last_step = None;
    }

    /// The part to apply in the frame drawn at `now`.
    pub fn step(&mut self, now: Instant) -> f32 {
        if !self.is_active() {
            self.last_step = None;
            return 0.;
        }
        let seconds = self
            .last_step
            .map_or(FIRST_FRAME_SECONDS, |last| (now - last).as_secs_f32())
            .min(LONGEST_FRAME_SECONDS);
        self.last_step = Some(now);
        self.take(seconds)
    }

    /// The part of what's left applied over `seconds`.
    fn take(&mut self, seconds: f32) -> f32 {
        if self.pending.abs() < LAST_PIXEL {
            return std::mem::take(&mut self.pending);
        }
        // egui's `exponential_smooth_factor`.
        let share = 1. - (1. - EASE_SHARE).powf(seconds / EASE_SECONDS);
        let applied = self.pending * share;
        self.pending -= applied;
        applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px};

    #[test]
    fn a_notch_is_mostly_applied_within_a_tenth_of_a_second() {
        let mut scroll = SmoothScroll::default();
        assert_eq!(scroll.push(40., true), 0.);
        let frame = 1. / 60.;
        let applied: f32 = (0..6).map(|_| scroll.take(frame)).sum();
        assert!((applied - 36.).abs() < 0.5, "{applied}");
        // Then the rest, down to nothing.
        let rest: f32 = (0..60).map(|_| scroll.take(frame)).sum();
        assert!((applied + rest - 40.).abs() < 1e-4);
        assert!(!scroll.is_active());
    }

    #[test]
    fn trackpad_steps_apply_at_once() {
        let mut scroll = SmoothScroll::default();
        assert_eq!(scroll.push(3., false), 3.);
        assert!(!scroll.is_active());
    }

    #[test]
    fn wheels_are_eased_and_trackpads_not() {
        let lines = WheelDelta::of(ScrollDelta::Lines(point(0., -1.)));
        assert_eq!((lines.vertical, lines.eased), (-40., true));
        let small = WheelDelta::of(ScrollDelta::Pixels(point(px(0.), px(3.))));
        assert!(!small.eased);
        let large = WheelDelta::of(ScrollDelta::Pixels(point(px(0.), px(14.))));
        assert!(large.eased);
    }
}
