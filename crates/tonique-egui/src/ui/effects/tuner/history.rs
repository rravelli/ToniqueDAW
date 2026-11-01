//! The tuner's history: how many cents off the notes were over the last
//! seconds, newest at the right, each note named where it starts.

use super::{IN_TUNE, Note, note};
use crate::ui::theme::{Theme, font_size};
use egui::{Align2, FontId, Painter, Rect, Stroke, pos2};
use std::collections::VecDeque;

/// Seconds shown.
const SPAN: f64 = 8.;
/// Cents from the middle to the top and bottom.
const RANGE: f32 = 50.;
/// Above the plot: the view buttons and the current note.
const HEADER: f32 = 18.;

/// Pitches heard, in Hz, by the time they were (`None` for none).
#[derive(Default)]
pub struct History {
    points: VecDeque<(f64, Option<f32>)>,
}

impl History {
    /// Add the pitch heard at `now`, and forget what's out of view.
    pub fn push(&mut self, now: f64, hz: Option<f32>) {
        if self.points.back().is_some_and(|&(t, _)| t >= now) {
            return;
        }
        self.points.push_back((now, hz));
        // One past the left edge, for the line to run to it.
        while self.points.get(1).is_some_and(|&(t, _)| t < now - SPAN) {
            self.points.pop_front();
        }
    }
}

/// Paint `history` in `rect` as of `now`, its notes against A4 at
/// `reference` Hz.
pub fn paint(
    painter: &Painter,
    rect: Rect,
    history: &History,
    now: f64,
    reference: f32,
    theme: &Theme,
) {
    let plot = Rect::from_min_max(pos2(rect.left(), rect.top() + HEADER), rect.max);
    let y = |cents: f32| plot.center().y - cents.clamp(-RANGE, RANGE) / RANGE * plot.height() / 2.;
    let x = |t: f64| plot.right() - ((now - t) / SPAN) as f32 * plot.width();

    // In tune in the middle, a quarter tone off at the lines.
    let band = Rect::from_x_y_ranges(plot.x_range(), y(IN_TUNE)..=y(-IN_TUNE));
    painter.rect_filled(band, 1., theme.success.gamma_multiply(0.12));
    for cents in [-25., 25.] {
        painter.hline(plot.x_range(), y(cents), Stroke::new(1., theme.grid_beat));
    }
    painter.hline(
        plot.x_range(),
        y(0.),
        Stroke::new(1., theme.success.gamma_multiply(0.4)),
    );
    let small = FontId::proportional(font_size::SMALL);
    for (cents, label) in [(25., "+25"), (-25., "-25")] {
        painter.text(
            pos2(plot.left() + 1., y(cents) - 1.),
            Align2::LEFT_BOTTOM,
            label,
            small.clone(),
            theme.text_disabled,
        );
    }

    let trace = painter.with_clip_rect(plot);
    let notes = history
        .points
        .iter()
        .map(|&(t, hz)| (t, hz.map(|hz| note(hz, reference))));
    let mut previous: Option<(f64, Note)> = None;
    for (t, heard) in notes {
        let Some(n) = heard else {
            previous = None;
            continue;
        };
        let at = pos2(x(t), y(n.cents));
        match previous {
            Some((pt, p)) if p.same(&n) => {
                let color = if n.in_tune() {
                    theme.success
                } else {
                    theme.accent
                };
                trace.line_segment([pos2(x(pt), y(p.cents)), at], Stroke::new(1.5, color));
            }
            // A new note: named where it starts.
            _ => {
                trace.vline(at.x, plot.y_range(), Stroke::new(1., theme.grid_beat));
                trace.text(
                    pos2(at.x + 3., plot.bottom() - 1.),
                    Align2::LEFT_BOTTOM,
                    format!("{}{}", n.name, n.octave),
                    small.clone(),
                    theme.text_muted,
                );
            }
        }
        previous = Some((t, n));
    }

    // The note now, at the top right.
    let current = history
        .points
        .back()
        .and_then(|&(_, hz)| hz)
        .map(|hz| note(hz, reference));
    let (text, color) = match current {
        Some(n) => (
            format!("{}{}  {:+.0} ct", n.name, n.octave, n.cents),
            if n.in_tune() {
                theme.success
            } else {
                theme.text
            },
        ),
        None => ("No pitch".into(), theme.text_disabled),
    };
    painter.text(
        pos2(rect.right() - 2., rect.top() + HEADER / 2. - 1.),
        Align2::RIGHT_CENTER,
        text,
        FontId::proportional(font_size::BODY),
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_what_is_in_view_and_one_before() {
        let mut history = History::default();
        for i in 0..=100 {
            history.push(i as f64 * 0.1, Some(440.));
        }
        let first = history.points.front().map(|&(t, _)| t).unwrap_or_default();
        assert!(first <= 10. - SPAN && first > 10. - SPAN - 0.15, "{first}");
        // The same time twice: kept once.
        history.push(10., None);
        assert_eq!(history.points.back(), Some(&(10., Some(440.))));
    }
}
