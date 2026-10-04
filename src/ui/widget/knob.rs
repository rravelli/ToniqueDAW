//! A knob: drag up or down to change a value, on a linear or log scale.

use crate::ui::theme::ThemeExt;
use egui::{
    Align2, CursorIcon, FontId, Pos2, Response, Sense, Shape, Stroke, Ui, Vec2, Widget, vec2,
};
use std::{
    f32::consts::{FRAC_PI_4, PI},
    ops::RangeInclusive,
};

const RADIUS: f32 = 11.;
const SIZE: Vec2 = vec2(Knob::WIDTH, Knob::HEIGHT);
/// Drag distance covering the whole range, in points. Shift drags ten
/// times finer.
const DRAG_RANGE: f32 = 150.;
/// Where the arc starts (the minimum), clockwise from the right; it sweeps
/// 270° to the maximum.
const START_ANGLE: f32 = 3. * FRAC_PI_4;
const SWEEP: f32 = 1.5 * PI;

/// Knob with its name above and its value below. The response is marked
/// changed when the value is. Double-click resets it to its default.
pub struct Knob<'a> {
    value: &'a mut f32,
    range: RangeInclusive<f32>,
    log: bool,
    default: Option<f32>,
    name: &'a str,
    format: &'a dyn Fn(f32) -> String,
    active: bool,
}

impl<'a> Knob<'a> {
    /// Room for its name and value.
    pub const WIDTH: f32 = 46.;
    /// Name, knob and value.
    pub const HEIGHT: f32 = 52.;

    pub fn new(value: &'a mut f32, range: RangeInclusive<f32>) -> Self {
        Self {
            value,
            range,
            log: false,
            default: None,
            name: "",
            format: &|v| format!("{v:.2}"),
            active: true,
        }
    }

    /// Logarithmic scale, for frequencies and times. The range must be
    /// positive.
    pub fn log(mut self, log: bool) -> Self {
        self.log = log;
        self
    }

    pub fn default(mut self, default: Option<f32>) -> Self {
        self.default = default;
        self
    }

    pub fn name(mut self, name: &'a str) -> Self {
        self.name = name;
        self
    }

    /// How the value is shown.
    pub fn format(mut self, format: &'a dyn Fn(f32) -> String) -> Self {
        self.format = format;
        self
    }

    /// Greyed out when not, as on a bypassed effect. Still editable.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    fn ratio(&self, value: f32) -> f32 {
        let (min, max) = (*self.range.start(), *self.range.end());
        let ratio = if self.log {
            (value / min).ln() / (max / min).ln()
        } else {
            (value - min) / (max - min)
        };
        if ratio.is_finite() {
            ratio.clamp(0., 1.)
        } else {
            0.
        }
    }

    fn value(&self, ratio: f32) -> f32 {
        let (min, max) = (*self.range.start(), *self.range.end());
        if self.log {
            min * (max / min).powf(ratio)
        } else {
            min + (max - min) * ratio
        }
    }
}

impl Widget for Knob<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, mut response) = ui.allocate_exact_size(SIZE, Sense::click_and_drag());
        let mut ratio = self.ratio(*self.value);
        if response.dragged() {
            let speed = if ui.input(|i| i.modifiers.shift) {
                0.1
            } else {
                1.
            };
            let delta = -response.drag_delta().y / DRAG_RANGE * speed;
            if delta != 0. {
                ratio = (ratio + delta).clamp(0., 1.);
                *self.value = self.value(ratio);
                response.mark_changed();
            }
        }
        if response.double_clicked()
            && let Some(default) = self.default
        {
            *self.value = default;
            ratio = self.ratio(default);
            response.mark_changed();
        }
        let response = response.on_hover_and_drag_cursor(CursorIcon::ResizeVertical);

        let theme = ui.app_theme();
        let painter = ui.painter();
        let font = FontId::proportional(9.);
        painter.text(
            Pos2::new(rect.center().x, rect.top()),
            Align2::CENTER_TOP,
            self.name,
            font.clone(),
            theme.text_muted,
        );
        let center = rect.center();
        let arc = |from: f32, to: f32| -> Vec<Pos2> {
            let steps = ((to - from) * 12.).ceil().max(1.) as usize;
            (0..=steps)
                .map(|i| {
                    let angle =
                        START_ANGLE + (from + (to - from) * i as f32 / steps as f32) * SWEEP;
                    center + RADIUS * Vec2::angled(angle)
                })
                .collect()
        };
        let color = if self.active {
            theme.accent
        } else {
            theme.text_disabled
        };
        painter.add(Shape::line(arc(0., 1.), Stroke::new(3., theme.bg_deep)));
        if ratio > 0. {
            painter.add(Shape::line(arc(0., ratio), Stroke::new(3., color)));
        }
        let fill = if response.hovered() || response.dragged() {
            theme.bg_control_hover
        } else {
            theme.bg_control
        };
        painter.circle_filled(center, RADIUS - 3., fill);
        let pointer = Vec2::angled(START_ANGLE + ratio * SWEEP);
        painter.line_segment(
            [center + pointer * 2., center + pointer * (RADIUS - 3.)],
            Stroke::new(1.5, theme.text),
        );
        painter.text(
            Pos2::new(rect.center().x, rect.bottom()),
            Align2::CENTER_BOTTOM,
            (self.format)(*self.value),
            font,
            theme.text,
        );
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratios_map_back_to_values() {
        let mut value = 0.;
        let knob = Knob::new(&mut value, 20.0..=20_000.).log(true);
        assert!((knob.ratio(632.46) - 0.5).abs() < 1e-3);
        assert!((knob.value(0.5) - 632.46).abs() < 0.1);
        assert_eq!(knob.ratio(1.), 0., "clamped below");
        let knob = Knob::new(&mut value, 0.0..=1.);
        assert_eq!(knob.ratio(0.25), 0.25);
        assert_eq!(knob.value(0.75), 0.75);
    }
}
