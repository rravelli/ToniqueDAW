//! Painting the timeline grid and ruler labels for a [`GridService`].

use crate::ui::theme::font_size;
use egui::{Align2, Color32, FontId, Painter, Rect, Stroke, pos2};
use tonique_engine::time::BeatPos;

use crate::{
    core::grid::{GridLine, GridService},
    ui::theme::{Theme, with_alpha},
    utils::format_time,
};

/// Room left of a label, from its line.
const LABEL_INSET: f32 = 3.;
/// Least room between two labels: one that would come closer is left out.
const LABEL_MIN_GAP: f32 = 4.;

pub trait PaintGrid {
    /// Bar lines across a clip, and its beats when zoomed in enough to
    /// show them.
    fn paint_clip_grid(&self, painter: &Painter, viewport: Rect, rect: Rect, color: Color32);
    /// The timeline's lines at the current zoom.
    fn paint_grid(&self, painter: &Painter, viewport: Rect, theme: &Theme);
    /// Bar and beat numbers along the ruler's bottom, times along its top.
    fn paint_labels(&self, painter: &Painter, rect: Rect, bpm: f32, theme: &Theme);
    /// Bar and beat numbers only, over all of `rect`.
    fn paint_bar_labels(&self, painter: &Painter, rect: Rect, theme: &Theme);
}

impl PaintGrid for GridService {
    fn paint_clip_grid(&self, painter: &Painter, viewport: Rect, rect: Rect, color: Color32) {
        let (bar, pulse) = (self.beats_per_bar(), self.pulse());
        let step = self.line_step();
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
        let (from, to) = self.visible_beats_in(shown, viewport);
        for (at, line) in self.lines(from, to, step) {
            let color = match line {
                GridLine::Bar(_) => color,
                _ => with_alpha(color, color.a() / 2),
            };
            painter.vline(
                self.beats_to_x(at, viewport),
                rect.y_range(),
                Stroke::new(1.0, color),
            );
        }
    }

    fn paint_grid(&self, painter: &Painter, viewport: Rect, theme: &Theme) {
        let (from, to) = self.visible_beats(viewport);
        for (at, line) in self.lines(from, to, self.line_step()) {
            let (color, width) = match line {
                GridLine::Bar(_) => (theme.grid_bar, 2.0),
                GridLine::Beat { .. } => (theme.grid_beat, 1.0),
                GridLine::Division { .. } => {
                    (with_alpha(theme.grid_beat, theme.grid_beat.a() / 2), 1.0)
                }
            };
            painter.vline(
                self.beats_to_x(at, viewport),
                viewport.y_range(),
                Stroke::new(width, color),
            );
        }
    }

    fn paint_labels(&self, painter: &Painter, rect: Rect, bpm: f32, theme: &Theme) {
        // Bars on the upper half, time on the lower.
        let (bars, times) = rect.split_top_bottom_at_fraction(0.5);
        self.paint_bar_labels(painter, bars, theme);
        paint_time_labels(self, painter, times, bpm, theme);
    }

    fn paint_bar_labels(&self, painter: &Painter, rect: Rect, theme: &Theme) {
        let (from, to) = self.visible_beats(rect);
        // A line just left of the view can have its label in it.
        let from = from - self.width_to_beats(LABEL_INSET + 40.);
        let from = if self.allows_negative() {
            from
        } else {
            from.max(BeatPos::ZERO)
        };
        let step = self.label_step(from, to);
        let font = FontId::new(font_size::SMALL, egui::FontFamily::Proportional);
        let mut last_right = f32::NEG_INFINITY;
        for (at, line) in self.lines(from, to, step) {
            let x = self.beats_to_x(at, rect);
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

impl GridService {
    /// The beats `shown` covers, drawn in `viewport`'s coordinates.
    fn visible_beats_in(&self, shown: Rect, viewport: Rect) -> (BeatPos, BeatPos) {
        let from = self.x_to_beats(shown.left(), viewport);
        let from = if self.allows_negative() {
            from
        } else {
            from.max(BeatPos::ZERO)
        };
        (from, self.x_to_beats(shown.right(), viewport))
    }
}

/// Times at a "nice" step for the zoom (1, 2, 5, 10, 15, 30 s…), with
/// milliseconds once it's under a second.
fn paint_time_labels(grid: &GridService, painter: &Painter, rect: Rect, bpm: f32, theme: &Theme) {
    let step = grid.time_step(bpm);
    let step_ms = (step * 1000.).round() as i64;
    let seconds = |beats: BeatPos| beats.0 * 60. / bpm as f64;
    let (from, to) = grid.visible_beats(rect);
    let from = from - grid.width_to_beats(LABEL_INSET + 60.);
    let first = (seconds(from) * 1000. / step_ms as f64).floor() as i64;
    let last = (seconds(to) * 1000. / step_ms as f64).ceil() as i64;
    let font = FontId::new(font_size::SMALL, egui::FontFamily::Proportional);
    for n in first..=last {
        if n < 0 && !grid.allows_negative() {
            continue;
        }
        let ms = n * step_ms;
        let x = grid.beats_to_x(BeatPos(ms as f64 / 1000. * bpm as f64 / 60.), rect);
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
