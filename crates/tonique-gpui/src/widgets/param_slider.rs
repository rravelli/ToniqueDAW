//! Tonique's `ParamSlider` as track headers show it: a well filled in the
//! track's colour (from the left for a gain, from the centre for a pan),
//! the value on top, dragged relative to where it was grabbed.

use gpui::{ContentMask, Window, px};

use crate::{
    geometry::Rect,
    theme::{Color, FONT_UI, Theme, hsla, size},
    utils::{format_pan, snap_to_centre},
    widgets::text::{paint_line, shape},
};

/// Shift-dragging moves this much slower.
const FINE_DRAG: f32 = 0.1;

/// What a slider sets, and how its position maps to the value.
#[derive(Clone, Copy, PartialEq)]
pub enum Scale {
    /// A linear gain shown in dB over `min..max`; its bottom is silence.
    Gain { min: f32, max: f32 },
    /// -1 (left) to 1 (right), snapping to the centre.
    Pan,
    /// 0 to 1, shown as a percentage, back to `default` on a double-click.
    Level { default: f32 },
    /// Whole steps from `min` to `max` (semitones, cents) in `unit`,
    /// filled from 0, back to 0 on a double-click.
    Steps {
        min: f32,
        max: f32,
        unit: &'static str,
    },
}

impl Scale {
    /// A track's volume.
    pub const VOLUME: Self = Self::Gain { min: -40., max: 5. };

    fn range(self) -> (f32, f32) {
        match self {
            Self::Gain { min, max } => (min, max),
            Self::Pan => (-1., 1.),
            Self::Level { .. } => (0., 1.),
            Self::Steps { min, max, .. } => (min, max),
        }
    }

    /// The value a double-click resets to.
    pub fn default_value(self) -> f32 {
        match self {
            Self::Gain { .. } => 1.,
            Self::Pan | Self::Steps { .. } => 0.,
            Self::Level { default } => default,
        }
    }

    /// The slider's position for a value.
    fn position(self, value: f32) -> f32 {
        match self {
            Self::Gain { min, max } if value > 0. => (20. * value.log10()).clamp(min, max),
            Self::Gain { min, .. } => min,
            Self::Pan => value.clamp(-1., 1.),
            Self::Level { .. } => value.clamp(0., 1.),
            Self::Steps { min, max, .. } => value.clamp(min, max),
        }
    }

    /// The value at a slider position.
    fn value(self, position: f32) -> f32 {
        match self {
            Self::Gain { min, .. } if position <= min => 0.,
            Self::Gain { .. } => 10f32.powf(position / 20.),
            Self::Pan => snap_to_centre(position),
            // In whole percents.
            Self::Level { .. } => (position * 100.).round() / 100.,
            Self::Steps { .. } => position.round(),
        }
    }

    fn text(self, value: f32, unit: bool) -> String {
        match self {
            Self::Gain { .. } => {
                let db = if value > 0. {
                    format!("{:.1}", 20. * value.log10())
                } else {
                    "-inf".into()
                };
                if unit { format!("{db} dB") } else { db }
            }
            Self::Pan => format_pan(value),
            Self::Level { .. } => format!("{:.0} %", value * 100.),
            // `+ 0.` shows -0 as 0.
            Self::Steps { unit, .. } => format!("{:+.0} {unit}", value + 0.),
        }
    }

    /// The part of `rect` filled for `value`.
    fn fill(self, rect: Rect, value: f32) -> Rect {
        let (min, max) = self.range();
        let at =
            |position: f32| rect.left + ((position - min) / (max - min)).clamp(0., 1.) * rect.width;
        let filled_x = at(self.position(value));
        let from = match self {
            Self::Gain { .. } | Self::Level { .. } => rect.left,
            Self::Pan | Self::Steps { .. } => at(0.),
        };
        Rect::new(
            from.min(filled_x),
            rect.top,
            (from - filled_x).abs(),
            rect.height,
        )
    }
}

/// A slider being dragged: the value follows the pointer's distance from
/// where it was anchored, so going past an end and back lands where it
/// left; Shift moves slower.
#[derive(Clone, Copy)]
pub struct SliderDrag {
    pub scale: Scale,
    /// The value when the drag started, for undo.
    pub start: f32,
    anchor_x: f32,
    anchor_position: f32,
    fine: bool,
    /// Whether the value changed.
    pub moved: bool,
}

impl SliderDrag {
    pub fn new(scale: Scale, value: f32, pointer_x: f32, fine: bool) -> Self {
        Self {
            scale,
            start: value,
            anchor_x: pointer_x,
            anchor_position: scale.position(value),
            fine,
            moved: false,
        }
    }

    /// The value with the pointer at `pointer_x` over a slider `width` wide, the
    /// value now being `current`; `None` while it doesn't change.
    pub fn follow(&mut self, pointer_x: f32, fine: bool, width: f32, current: f32) -> Option<f32> {
        // Toggling Shift re-anchors, so the value doesn't jump.
        if self.fine != fine {
            self.anchor_x = pointer_x;
            self.anchor_position = self.scale.position(current);
            self.fine = fine;
        }
        let (min, max) = self.scale.range();
        let speed = if self.fine { FINE_DRAG } else { 1. };
        let per_pixel = (max - min) / width.max(1.) * speed;
        let position =
            (self.anchor_position + (pointer_x - self.anchor_x) * per_pixel).clamp(min, max);
        let value = self.scale.value(position);
        (value != current).then(|| {
            self.moved = true;
            value
        })
    }
}

/// The slider over `rect` at `value`, filled in `color`; a gain with its unit
/// when `unit`. The value reads on the fill where it's over it and on the
/// well elsewhere.
#[allow(clippy::too_many_arguments)]
pub fn paint_param_slider(
    window: &mut Window,
    context: &mut gpui::App,
    rect: Rect,
    scale: Scale,
    value: f32,
    color: Color,
    unit: bool,
    theme: &Theme,
) {
    let filled = scale.fill(rect, value);
    window.paint_quad(gpui::fill(rect.bounds(), hsla(theme.bg_deep)).corner_radii(px(1.)));
    window.paint_quad(gpui::fill(filled.bounds(), hsla(color)).corner_radii(px(1.)));

    let text = scale.text(value, unit);
    let font = gpui::font(FONT_UI);
    let width = f32::from(
        shape(
            window,
            text.clone(),
            size::SMALL,
            hsla(theme.text),
            font.clone(),
        )
        .width,
    );
    let (text_left, text_top) = (
        rect.left + (rect.width - width) / 2.,
        rect.top + (rect.height - size::SMALL * 1.25) / 2.,
    );
    let parts = [
        (
            Rect::new(rect.left, rect.top, filled.left - rect.left, rect.height),
            theme.text,
        ),
        (
            Rect::new(
                filled.right(),
                rect.top,
                rect.right() - filled.right(),
                rect.height,
            ),
            theme.text,
        ),
        (filled, theme.text_on(color)),
    ];
    for (part, ink) in parts {
        if part.is_empty() {
            continue;
        }
        let line = shape(window, text.clone(), size::SMALL, hsla(ink), font.clone());
        let mask = ContentMask {
            bounds: part.bounds(),
        };
        window.with_content_mask(Some(mask), |window| {
            paint_line(window, context, &line, text_left, text_top, size::SMALL)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gains_map_to_decibels_down_to_silence() {
        let scale = Scale::VOLUME;
        assert_eq!(scale.position(1.), 0.);
        assert_eq!(scale.position(0.), -40.);
        assert_eq!(scale.value(-40.), 0.);
    }

    #[test]
    fn the_value_follows_the_pointer_past_the_ends() {
        // 45 dB over 90 px: 0.5 dB a pixel.
        let mut drag = SliderDrag::new(Scale::VOLUME, 1., 100., false);
        let db = |gain: f32| (20. * gain.log10() * 10.).round() / 10.;
        assert_eq!(drag.follow(104., false, 90., 1.).map(db), Some(2.));
        assert_eq!(drag.follow(300., false, 90., 1.).map(db), Some(5.));
        assert_eq!(drag.follow(104., false, 90., 1.78).map(db), Some(2.));
    }

    #[test]
    fn levels_step_by_whole_percents() {
        let level = Scale::Level { default: 0.5 };
        assert_eq!(level.value(0.123), 0.12);
        assert_eq!(level.text(0.75, false), "75 %");
        let fill = level.fill(Rect::new(0., 0., 100., 10.), 0.25);
        assert_eq!((fill.left, fill.width), (0., 25.));
    }

    #[test]
    fn steps_are_whole_and_signed() {
        let steps = Scale::Steps {
            min: -48.,
            max: 48.,
            unit: "st",
        };
        assert_eq!(steps.value(2.4), 2.);
        assert_eq!(steps.text(3., false), "+3 st");
        assert_eq!(steps.text(-0., false), "+0 st");
    }

    #[test]
    fn pans_fill_from_the_centre() {
        let rect = Rect::new(0., 0., 100., 10.);
        let left = Scale::Pan.fill(rect, -0.5);
        assert_eq!((left.left, left.width), (25., 25.));
        let right = Scale::Pan.fill(rect, 1.);
        assert_eq!((right.left, right.width), (50., 50.));
    }
}
