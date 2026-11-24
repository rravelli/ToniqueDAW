//! Painting the project's grid and the ruler's labels, at a view's zoom
//! and scroll.

use crate::ui::RectSpan;
use crate::ui::theme::font_size;
use egui::{Align2, Color32, FontId, Painter, Rect, Stroke, pos2};
use tonique_engine::time::BeatPos;

use crate::ui::theme::{Theme, with_alpha};
use tonique_core::{
    session::{
        grid::{Grid, GridLine},
        timeline_view::TimelineView,
    },
    utils::format_time,
};

/// Room left of a label, from its line.
const LABEL_INSET: f32 = 3.;
/// Least room between two labels: one that would come closer is left out.
const LABEL_MIN_GAP: f32 = 4.;

/// The project's grid as `view` shows it: what's painted.
#[derive(Clone, Copy)]
pub struct ZoomedGrid<'a> {
    pub grid: &'a Grid,
    pub view: &'a TimelineView,
}

impl<'a> ZoomedGrid<'a> {
    pub fn new(grid: &'a Grid, view: &'a TimelineView) -> Self {
        Self { grid, view }
    }

    /// Bar lines across a clip, and its beats when zoomed in enough to
    /// show them.
    pub fn paint_clip_grid(&self, painter: &Painter, viewport: Rect, rect: Rect, color: Color32) {
        let (bar, pulse) = (self.grid.beats_per_bar(), self.grid.pulse());
        let step = self.grid.line_step(self.view);
        if step >= bar {
            return;
        }
        let step = if step <= pulse { pulse } else { bar };
        let shown = Rect::from_x_y_ranges(
            rect.left().max(viewport.left())..=rect.right().min(viewport.right()),
            rect.y_range(),
        );
        if !shown.is_positive() {
            return;
        }
        let (from, to) = self.view.visible_beats_in(shown.span(), viewport.span());
        for (at, line) in self.grid.lines(from, to, step) {
            let color = match line {
                GridLine::Bar(_) => color,
                _ => with_alpha(color, color.a() / 2),
            };
            painter.vline(
                self.view.beats_to_x(at, viewport.span()),
                rect.y_range(),
                Stroke::new(1.0, color),
            );
        }
    }

    /// The timeline's lines at the current zoom.
    pub fn paint_grid(&self, painter: &Painter, viewport: Rect, theme: &Theme) {
        let (from, to) = self.view.visible_beats(viewport.span());
        for (at, line) in self.grid.lines(from, to, self.grid.line_step(self.view)) {
            let (color, width) = match line {
                GridLine::Bar(_) => (theme.grid_bar, 2.0),
                GridLine::Beat { .. } => (theme.grid_beat, 1.0),
                GridLine::Division { .. } => {
                    (with_alpha(theme.grid_beat, theme.grid_beat.a() / 2), 1.0)
                }
            };
            painter.vline(
                self.view.beats_to_x(at, viewport.span()),
                viewport.y_range(),
                Stroke::new(width, color),
            );
        }
    }

    /// Bar and beat numbers along the ruler's bottom, times along its top.
    pub fn paint_labels(&self, painter: &Painter, rect: Rect, bpm: f32, theme: &Theme) {
        // Bars on the upper half, time on the lower.
        let (bars, times) = rect.split_top_bottom_at_fraction(0.5);
        self.paint_bar_labels(painter, bars, theme);
        paint_time_labels(self.view, painter, times, bpm, theme);
    }

    /// Bar and beat numbers only, over all of `rect`.
    pub fn paint_bar_labels(&self, painter: &Painter, rect: Rect, theme: &Theme) {
        let (from, to) = self.view.visible_beats(rect.span());
        // A line just left of the view can have its label in it.
        let from = from - self.view.width_to_beats(LABEL_INSET + 40.);
        let from = if self.view.allows_negative() {
            from
        } else {
            from.max(BeatPos::ZERO)
        };
        let step = self.grid.label_step(from, to, self.view);
        let font = FontId::new(font_size::SMALL, egui::FontFamily::Proportional);
        let mut last_right = f32::NEG_INFINITY;
        for (at, line) in self.grid.lines(from, to, step) {
            let x = self.view.beats_to_x(at, rect.span());
            let (text, top) = match line {
                GridLine::Bar(bar) => (bar.to_string(), rect.top()),
                GridLine::Beat { bar, beat } => (format!("{bar}.{beat}"), rect.center().y),
                GridLine::Division {
                    bar,
                    beat,
                    sub: Some(sub),
                } => (format!("{bar}.{beat}.{sub}"), rect.center().y),
                GridLine::Division { sub: None, .. } => continue,
            };
            let width = if matches!(line, GridLine::Bar(_)) {
                2.0
            } else {
                1.0
            };
            painter.vline(
                x,
                top..=rect.bottom(),
                Stroke::new(width, theme.text_disabled),
            );
            // Never over the one before, whatever the estimate said.
            let galley = painter.layout_no_wrap(text, font.clone(), theme.text_muted);
            let left = x + LABEL_INSET;
            if left < last_right + LABEL_MIN_GAP {
                continue;
            }
            last_right = left + galley.size().x;
            painter.galley(
                pos2(left, rect.center().y - galley.size().y / 2.),
                galley,
                theme.text_muted,
            );
        }
    }
}

/// Times at a "nice" step for the zoom (1, 2, 5, 10, 15, 30 s…), with
/// milliseconds once it's under a second.
fn paint_time_labels(view: &TimelineView, painter: &Painter, rect: Rect, bpm: f32, theme: &Theme) {
    let step = view.time_step(bpm);
    let step_ms = (step * 1000.).round() as i64;
    let seconds = |beats: BeatPos| beats.0 * 60. / bpm as f64;
    let (from, to) = view.visible_beats(rect.span());
    let from = from - view.width_to_beats(LABEL_INSET + 60.);
    let first = (seconds(from) * 1000. / step_ms as f64).floor() as i64;
    let last = (seconds(to) * 1000. / step_ms as f64).ceil() as i64;
    let font = FontId::new(font_size::SMALL, egui::FontFamily::Proportional);
    for n in first..=last {
        if n < 0 && !view.allows_negative() {
            continue;
        }
        let ms = n * step_ms;
        let x = view.beats_to_x(BeatPos(ms as f64 / 1000. * bpm as f64 / 60.), rect.span());
        let text = if step < 1. {
            format_time(ms as f64 / 1000.)
        } else {
            format_clock(ms / 1000)
        };
        painter.text(
            pos2(x + LABEL_INSET, rect.center().y),
            Align2::LEFT_CENTER,
            text,
            font.clone(),
            theme.text_disabled,
        );
        painter.vline(
            x,
            rect.center().y..=rect.bottom(),
            Stroke::new(1.0, theme.text_disabled),
        );
    }
}

/// Whole `seconds` as `m:ss`, with a `-` before zero.
fn format_clock(seconds: i64) -> String {
    let sign = if seconds < 0 { "-" } else { "" };
    let seconds = seconds.unsigned_abs();
    format!("{sign}{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::format_clock;

    #[test]
    fn clock_times_count_minutes_past_the_hour() {
        assert_eq!(format_clock(0), "0:00");
        assert_eq!(format_clock(75), "1:15");
        assert_eq!(format_clock(-5), "-0:05");
        assert_eq!(format_clock(3725), "62:05");
    }
}
