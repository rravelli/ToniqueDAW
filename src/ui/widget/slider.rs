use egui::{Align2, CursorIcon, FontId, Response, Sense, Ui, Widget, emath::Numeric, pos2, vec2};

use crate::ui::theme::ThemeExt;

const HEIGHT: f32 = 18.;
const TRACK_HEIGHT: f32 = 4.;
const HANDLE_RADIUS: f32 = 5.;
const VALUE_WIDTH: f32 = 52.;

/// Horizontal slider with its value printed on the right. Clicking or
/// dragging sets the value under the pointer; double-clicking restores the
/// default, if any. Check `drag_stopped` to apply costly changes once.
pub struct ValueSlider<'a, N: Numeric> {
    value: &'a mut N,
    min: f64,
    max: f64,
    step: Option<f64>,
    default: Option<N>,
    width: f32,
    formatter: Box<dyn Fn(f64) -> String + 'a>,
}

impl<'a, N: Numeric> ValueSlider<'a, N> {
    pub fn new(value: &'a mut N, range: std::ops::RangeInclusive<N>) -> Self {
        Self {
            value,
            min: range.start().to_f64(),
            max: range.end().to_f64(),
            step: N::INTEGRAL.then_some(1.),
            default: None,
            width: 200.,
            formatter: Box::new(|v| {
                if N::INTEGRAL {
                    format!("{v:.0}")
                } else {
                    format!("{v:.2}")
                }
            }),
        }
    }

    pub fn step(mut self, step: f64) -> Self {
        self.step = Some(step);
        self
    }

    pub fn default_value(mut self, value: N) -> Self {
        self.default = Some(value);
        self
    }

    /// Total width, value text included.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn formatter(mut self, formatter: impl Fn(f64) -> String + 'a) -> Self {
        self.formatter = Box::new(formatter);
        self
    }

    /// Show the value as a percentage (1 = 100%).
    pub fn percent(self) -> Self {
        self.formatter(|v| format!("{:.0}%", v * 100.))
    }

    fn ratio(&self) -> f32 {
        if self.max > self.min {
            ((self.value.to_f64() - self.min) / (self.max - self.min)).clamp(0., 1.) as f32
        } else {
            0.
        }
    }

    fn set_ratio(&mut self, ratio: f32) {
        let mut value = self.min + ratio.clamp(0., 1.) as f64 * (self.max - self.min);
        if let Some(step) = self.step {
            value = self.min + ((value - self.min) / step).round() * step;
        }
        *self.value = N::from_f64(value.clamp(self.min, self.max));
    }
}

impl<N: Numeric> Widget for ValueSlider<'_, N> {
    fn ui(mut self, ui: &mut Ui) -> Response {
        let enabled = ui.is_enabled();
        let (rect, mut response) =
            ui.allocate_exact_size(vec2(self.width, HEIGHT), Sense::click_and_drag());
        let track = egui::Rect::from_min_max(
            pos2(
                rect.left() + HANDLE_RADIUS,
                rect.center().y - TRACK_HEIGHT / 2.,
            ),
            pos2(
                rect.right() - VALUE_WIDTH - HANDLE_RADIUS,
                rect.center().y + TRACK_HEIGHT / 2.,
            ),
        );

        let before = *self.value;
        if response.double_clicked()
            && let Some(default) = self.default
        {
            *self.value = default;
        } else if (response.dragged() || response.clicked())
            && let Some(pointer) = response.interact_pointer_pos()
        {
            self.set_ratio((pointer.x - track.left()) / track.width());
        }
        if *self.value != before {
            response.mark_changed();
        }

        let theme = ui.app_theme();
        let active = response.dragged() || response.hovered();
        let fill = match (enabled, active) {
            (false, _) => theme.text_disabled,
            (true, true) => theme.accent_hover,
            (true, false) => theme.accent,
        };
        let painter = ui.painter();
        let handle_x = track.left() + self.ratio() * track.width();
        painter.rect_filled(track, TRACK_HEIGHT / 2., theme.bg_control);
        painter.rect_filled(track.with_max_x(handle_x), TRACK_HEIGHT / 2., fill);
        painter.circle_filled(
            pos2(handle_x, track.center().y),
            if active && enabled {
                HANDLE_RADIUS + 1.
            } else {
                HANDLE_RADIUS
            },
            if enabled {
                theme.text
            } else {
                theme.text_disabled
            },
        );
        painter.text(
            rect.right_center(),
            Align2::RIGHT_CENTER,
            (self.formatter)(self.value.to_f64()),
            FontId::proportional(11.),
            if enabled {
                theme.text_muted
            } else {
                theme.text_disabled
            },
        );

        if enabled {
            response = response.on_hover_cursor(CursorIcon::PointingHand);
        }
        response
    }
}
