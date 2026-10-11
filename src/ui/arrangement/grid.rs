//! Painting the timeline grid and ruler labels for a [`GridService`].

use crate::ui::theme::font_size;
use egui::{Align2, Color32, FontId, Painter, Rect, Stroke, pos2};

use crate::{
    core::grid::{GridResolution, GridService},
    ui::theme::{Theme, with_alpha},
};

pub trait PaintGrid {
    /// Bar lines across a clip, when zoomed in enough to show beats.
    fn paint_clip_grid(&self, painter: &Painter, viewport: Rect, rect: Rect, color: Color32);
    /// The timeline's lines at the current resolution.
    fn paint_grid(&self, painter: &Painter, viewport: Rect, theme: &Theme);
    /// Bar and beat numbers along the ruler's bottom, times along its top.
    fn paint_labels(&self, painter: &Painter, rect: Rect, bpm: f32, theme: &Theme);
    /// Bar and beat numbers only, over all of `rect`.
    fn paint_bar_labels(&self, painter: &Painter, rect: Rect, theme: &Theme);
}

impl PaintGrid for GridService {
    fn paint_clip_grid(&self, painter: &Painter, viewport: Rect, rect: Rect, color: Color32) {
        if self.resolution().divisions_per_beat(self.beats_per_bar())
            <= GridResolution::Bar.divisions_per_beat(self.beats_per_bar())
        {
            return;
        }
        let divisions_per_beat = GridResolution::Bar.divisions_per_beat(self.beats_per_bar());

        let step = self.pixels_per_beat() / divisions_per_beat; // pixel spacing between grid lines
        let clip_offset = self.offset.x + rect.left() - viewport.left();
        let mut step_index = (clip_offset / step).floor() as i32;
        let mut x = rect.left() - clip_offset.rem_euclid(step);
        // Jump to the first visible line
        let skipped = ((viewport.left() - x) / step).floor().max(0.);
        x += skipped * step;
        step_index += skipped as i32;
        let right = rect.right().min(viewport.right());
        while x < right {
            // Skip if out of bounds
            if step_index < 0 && !self.allows_negative() {
                step_index += 1;
                x += step;
                continue;
            }

            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1.0, color),
            );

            step_index += 1;
            x += step;
        }
    }

    fn paint_grid(&self, painter: &Painter, viewport: Rect, theme: &Theme) {
        let divisions_per_beat = self.resolution().divisions_per_beat(self.beats_per_bar());
        let step = self.pixels_per_beat() / divisions_per_beat; // pixel spacing between grid lines

        let mut step_index = (self.offset.x / step).floor() as i32;
        let right = viewport.right();
        let mut x = viewport.left() - self.offset.x.rem_euclid(step);

        let lines_per_bar = (self.beats_per_bar() as f32 * divisions_per_beat).max(1.0) as i32;
        let lines_per_beat = divisions_per_beat.max(1.0) as i32;

        while x < right {
            // Skip if out of bounds
            if step_index < 0 && !self.allows_negative() {
                step_index += 1;
                x += step;
                continue;
            }
            let is_bar = step_index % lines_per_bar == 0;
            let is_major_beat = step_index % lines_per_beat == 0;

            let color = if is_bar {
                theme.grid_bar
            } else if is_major_beat {
                theme.grid_beat
            } else {
                with_alpha(theme.grid_beat, theme.grid_beat.a() / 2)
            };

            painter.line_segment(
                [pos2(x, viewport.top()), pos2(x, viewport.bottom())],
                Stroke::new(if is_bar { 2.0 } else { 1.0 }, color),
            );

            step_index += 1;
            x += step;
        }
    }

    fn paint_labels(&self, painter: &Painter, rect: Rect, bpm: f32, theme: &Theme) {
        // Bars on the upper half, time on the lower.
        let (bars, times) = rect.split_top_bottom_at_fraction(0.5);
        self.paint_bar_labels(painter, bars, theme);
        paint_time_labels(self, painter, times, bpm, theme);
    }

    fn paint_bar_labels(&self, painter: &Painter, rect: Rect, theme: &Theme) {
        let divisions_per_beat = self.resolution().divisions_per_beat(self.beats_per_bar());
        let step = self.pixels_per_beat() / divisions_per_beat; // pixel spacing between grid lines

        let mut step_index = (self.offset.x / step).floor() as i32;
        let right = rect.right();
        let mut x = rect.left() - self.offset.x.rem_euclid(step);

        let lines_per_bar = (self.beats_per_bar() as f32 * divisions_per_beat).max(1.0) as i32;
        let lines_per_beat = divisions_per_beat.max(1.0) as i32;

        while x < right {
            // Skip if out of bounds
            if step_index < 0 && !self.allows_negative() {
                step_index += 1;
                x += step;
                continue;
            }

            let is_bar = step_index % lines_per_bar == 0;
            let is_major_beat = step_index % lines_per_beat == 0;

            let bar_index = bar_number(step_index.div_euclid(lines_per_bar));
            if is_bar {
                let text = format!("{}", bar_index);
                painter.text(
                    pos2(x + 3.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    text,
                    FontId::new(font_size::SMALL, egui::FontFamily::Monospace),
                    theme.text_muted,
                );
                painter.line_segment(
                    [pos2(x, rect.top()), pos2(x, rect.bottom())],
                    Stroke::new(2.0, theme.text_disabled),
                );
            }
            let beat_index = step_index
                .div_euclid(lines_per_beat)
                .rem_euclid(self.beats_per_bar() as i32)
                + 1;
            if divisions_per_beat
                >= GridResolution::Quarter.divisions_per_beat(self.beats_per_bar())
                && is_major_beat
            {
                let text = format!("{}.{}", bar_index, beat_index);
                painter.text(
                    pos2(x + 3.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    text,
                    FontId::new(font_size::SMALL, egui::FontFamily::Monospace),
                    theme.text_muted,
                );
                painter.line_segment(
                    [pos2(x, rect.center().y), pos2(x, rect.bottom())],
                    Stroke::new(1.0, theme.text_disabled),
                );
            }

            step_index += 1;
            x += step;
        }
    }
}

fn paint_time_labels(grid: &GridService, painter: &Painter, rect: Rect, bpm: f32, theme: &Theme) {
    let seconds_step = grid.resolution().step_size_secs();
    let step = grid.pixels_per_beat() * bpm / 60. * seconds_step;
    let mut step_index = (grid.offset.x / step).floor() as i32;
    let mut x = rect.left() - grid.offset.x.rem_euclid(step);

    while x < rect.right() {
        // Skip if out of bounds
        if step_index < 0 && !grid.allows_negative() {
            step_index += 1;
            x += step;
            continue;
        }
        let time = (step_index as f32 * seconds_step).floor() as i32;
        let sign = if time < 0 { "-" } else { "" };
        let time = time.abs();
        let seconds = time % 60;
        let minutes = (time / 60) % 60;
        let text = format!("{sign}{}:{:0>2}", minutes, seconds);
        painter.text(
            pos2(x + 3.0, rect.center().y),
            Align2::LEFT_CENTER,
            text,
            FontId::new(font_size::SMALL, egui::FontFamily::Monospace),
            theme.text_disabled,
        );
        painter.line_segment(
            [pos2(x, rect.center().y), pos2(x, rect.bottom())],
            Stroke::new(1.0, theme.text_disabled),
        );
        step_index += 1;
        x += step;
    }
}

/// The number shown for the bar at `index` from beat 0: from 1, and before
/// beat 0 from -1 down (there's no bar 0).
fn bar_number(index: i32) -> i32 {
    if index >= 0 { index + 1 } else { index }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_before_the_start_count_down_from_minus_one() {
        assert_eq!(bar_number(0), 1);
        assert_eq!(bar_number(3), 4);
        assert_eq!(bar_number(-1), -1);
        assert_eq!(bar_number(-2), -2);
    }
}
