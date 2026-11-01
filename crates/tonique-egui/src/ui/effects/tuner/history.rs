//! The tuner's history: the pitches of the last seconds, newest at the
//! right, on a lane per note named at the left. The lanes follow the notes
//! in view.

use super::{NAMES, note};
use crate::ui::theme::{Theme, font_size};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2};
use std::collections::VecDeque;

/// Seconds shown.
const SPAN: f64 = 8.;
/// Above the plot: the view buttons and the current note.
const HEADER: f32 = 18.;
/// Left of the plot: the lanes' names.
const LABELS: f32 = 28.;
/// Lanes shown at least: an octave, both ends included.
const MIN_LANES: f32 = 13.;
/// Lanes kept above the highest note in view and below the lowest.
const MARGIN: f32 = 2.;
/// Time for the lanes shown to cover ~63% of the way to new ones, in
/// seconds.
const EASE: f32 = 0.25;
/// The lane in the middle before anything is heard: A4.
const START: f32 = 69.;
/// Lanes at least this tall, in points, all have their names; shorter,
/// only the Cs do.
const NAMED_LANE: f32 = font_size::SMALL + 1.;

/// Pitches heard, in Hz, by the time they were (`None` for none), and the
/// lanes shown.
#[derive(Default)]
pub struct History {
    points: VecDeque<(f64, Option<f32>)>,
    /// The lowest and highest lanes shown, as MIDI notes; eased, so not
    /// whole.
    lanes: Option<(f32, f32)>,
}

/// `hz` as a MIDI note, A4 being at `reference` Hz: whole on a note,
/// fractional between them.
fn midi(hz: f32, reference: f32) -> f32 {
    69. + 12. * (hz / reference).log2()
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

    /// Move the lanes shown towards the notes in view, `dt` seconds after
    /// the last time.
    pub fn follow(&mut self, reference: f32, dt: f32) {
        let since = self.points.back().map_or(0., |&(t, _)| t - SPAN);
        let (low, high) = self
            .points
            .iter()
            .filter(|&&(t, _)| t >= since)
            .filter_map(|&(_, hz)| hz)
            .map(|hz| midi(hz, reference).round())
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), m| {
                (lo.min(m), hi.max(m))
            });
        let (low, high) = if low.is_finite() {
            (low - MARGIN, high + MARGIN)
        } else {
            self.lanes.unwrap_or((START, START))
        };
        let short = (MIN_LANES - 1. - (high - low)).max(0.) / 2.;
        let target = (low - short, high + short);
        self.lanes = Some(match self.lanes {
            Some((lo, hi)) => {
                let k = 1. - (-dt / EASE).exp();
                (lo + (target.0 - lo) * k, hi + (target.1 - hi) * k)
            }
            None => target,
        });
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
    let plot = Rect::from_min_max(pos2(rect.left() + LABELS, rect.top() + HEADER), rect.max);
    let (low, high) = history.lanes.unwrap_or((START - 6., START + 6.));
    let (bottom, top) = (low - 0.5, high + 0.5);
    let lane_height = plot.height() / (top - bottom);
    let y = |m: f32| plot.bottom() - (m - bottom) * lane_height;
    let x = |t: f64| plot.right() - ((now - t) / SPAN) as f32 * plot.width();
    let current = history.points.back().and_then(|&(_, hz)| hz);
    let current_lane = current.map(|hz| midi(hz, reference).round() as i32);
    let current_color = match current {
        Some(hz) if note(hz, reference).in_tune() => theme.success,
        _ => theme.accent,
    };

    let lanes = painter.with_clip_rect(plot);
    let labels = painter.with_clip_rect(Rect::from_min_max(
        pos2(rect.left(), plot.top()),
        pos2(plot.left(), plot.bottom()),
    ));
    let small = FontId::proportional(font_size::SMALL);
    for lane in bottom.floor() as i32..=top.ceil() as i32 {
        let name = NAMES[lane.rem_euclid(12) as usize];
        let center = y(lane as f32);
        let band = Rect::from_x_y_ranges(
            plot.x_range(),
            center - lane_height / 2.0..=center + lane_height / 2.,
        );
        let is_current = current_lane == Some(lane);
        if is_current {
            lanes.rect_filled(band, 0., current_color.gamma_multiply(0.12));
        } else if name.ends_with('#') {
            lanes.rect_filled(band, 0., theme.text.gamma_multiply(0.03));
        }
        // Octaves apart under each C.
        if name == "C" {
            lanes.hline(
                plot.x_range(),
                band.bottom(),
                Stroke::new(1., theme.grid_bar),
            );
        }
    }

    // Names, the most useful first, each where it doesn't cover another:
    // the note now, the notes played, the Cs, then the rest if roomy.
    let since = now - SPAN;
    let played: Vec<i32> = history
        .points
        .iter()
        .filter(|&&(t, _)| t >= since)
        .filter_map(|&(_, hz)| hz)
        .map(|hz| midi(hz, reference).round() as i32)
        .collect();
    let shown = bottom.floor() as i32..=top.ceil() as i32;
    // With their octave, but for the rest.
    let mut order: Vec<(i32, Color32, bool)> = current_lane
        .map(|lane| (lane, current_color, true))
        .into_iter()
        .chain(played.iter().map(|&lane| (lane, theme.text_muted, true)))
        .chain(
            shown
                .clone()
                .filter(|l| l.rem_euclid(12) == 0)
                .map(|l| (l, theme.text_muted, true)),
        )
        .chain(shown.map(|l| (l, theme.text_disabled, false)))
        .collect();
    if lane_height < NAMED_LANE {
        order.retain(|&(_, _, octave)| octave);
    }
    let mut named: Vec<f32> = Vec::new();
    for (lane, color, octave) in order {
        let center = y(lane as f32);
        if named
            .iter()
            .any(|&other| (other - center).abs() < NAMED_LANE)
        {
            continue;
        }
        named.push(center);
        let name = NAMES[lane.rem_euclid(12) as usize];
        let text = if octave {
            format!("{name}{}", lane.div_euclid(12) - 1)
        } else {
            name.to_owned()
        };
        labels.text(
            pos2(rect.left() + 2., center),
            Align2::LEFT_CENTER,
            text,
            small.clone(),
            color,
        );
    }

    // The trace, a line per run of one colour, broken where nothing was
    // heard and across leaps.
    let mut run: Vec<Pos2> = Vec::new();
    let mut run_color = theme.accent;
    let mut previous: Option<f32> = None;
    for &(t, heard) in &history.points {
        let point = heard.map(|hz| (midi(hz, reference), note(hz, reference).in_tune()));
        let joined = matches!((previous, point), (Some(pm), Some((m, _))) if (m - pm).abs() < 0.5);
        if !joined {
            flush(&lanes, &mut run, run_color);
        }
        let Some((m, in_tune)) = point else {
            previous = None;
            continue;
        };
        let color = if in_tune { theme.success } else { theme.accent };
        let at = pos2(x(t), y(m));
        if color != run_color && !run.is_empty() {
            // The new colour carries on from the last point.
            let last = run[run.len() - 1];
            flush(&lanes, &mut run, run_color);
            run.push(last);
        }
        run_color = color;
        run.push(at);
        previous = Some(m);
    }
    flush(&lanes, &mut run, run_color);

    // The note now, at the top right.
    let (text, color) = match current.map(|hz| note(hz, reference)) {
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

/// Draw `run` as one line, and empty it.
fn flush(painter: &Painter, run: &mut Vec<Pos2>, color: Color32) {
    if run.len() > 1 {
        painter.add(Shape::line(std::mem::take(run), Stroke::new(2., color)));
    }
    run.clear();
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

    /// The lanes span the notes in view and a margin, at least an octave,
    /// and ease towards new ones.
    #[test]
    fn the_lanes_follow_the_notes() {
        let mut history = History::default();
        history.push(0., Some(440.));
        history.follow(440., 0.);
        assert_eq!(history.lanes, Some((63., 75.)), "an octave around A4");
        // E2 to A4: past an octave, so the margin alone.
        history.push(0.1, Some(82.41));
        history.follow(440., 10.);
        let (low, high) = history.lanes.unwrap_or_default();
        assert!((low - 38.).abs() < 0.01 && (high - 71.).abs() < 0.01);
        // Eased: a short step moves a little of the way.
        history.push(20., Some(440.));
        history.push(20.1, Some(440.));
        history.follow(440., 0.01);
        let (low, _) = history.lanes.unwrap_or_default();
        assert!(low > 38. && low < 45., "{low}");
        // Nothing heard: they stay.
        let before = history.lanes;
        history.push(40., None);
        history.push(40.1, None);
        history.follow(440., 1.);
        assert_eq!(history.lanes, before);
    }
}
