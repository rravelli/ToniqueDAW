//! The tuner's history, as the egui app's: the pitches of the last
//! seconds, newest at the right, on a lane per note named at the left. The
//! lanes follow the notes in view.

use std::collections::VecDeque;

use gpui::ContentMask;

use crate::{
    bottom::paint::{Align, fill, polyline, text},
    core::note::{NAMES, note},
    geometry::Rect,
    theme::{Color, Theme, faded, size},
};

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
/// Lanes at least this tall all have their names; shorter, only the Cs
/// do.
const NAMED_LANE: f32 = size::SMALL + 1.;

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
        if self.points.back().is_some_and(|&(time, _)| time >= now) {
            return;
        }
        self.points.push_back((now, hz));
        // One past the left edge, for the line to run to it.
        while self
            .points
            .get(1)
            .is_some_and(|&(time, _)| time < now - SPAN)
        {
            self.points.pop_front();
        }
    }

    /// Move the lanes shown towards the notes in view, `seconds` after the
    /// last time.
    pub fn follow(&mut self, reference: f32, seconds: f32) {
        let since = self.points.back().map_or(0., |&(time, _)| time - SPAN);
        let (low, high) = self
            .points
            .iter()
            .filter(|&&(time, _)| time >= since)
            .filter_map(|&(_, hz)| hz)
            .map(|hz| midi(hz, reference).round())
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), lane| {
                (low.min(lane), high.max(lane))
            });
        let (low, high) = if low.is_finite() {
            (low - MARGIN, high + MARGIN)
        } else {
            self.lanes.unwrap_or((START, START))
        };
        let short = (MIN_LANES - 1. - (high - low)).max(0.) / 2.;
        let target = (low - short, high + short);
        self.lanes = Some(match self.lanes {
            Some((low, high)) => {
                let step = 1. - (-seconds / EASE).exp();
                (
                    low + (target.0 - low) * step,
                    high + (target.1 - high) * step,
                )
            }
            None => target,
        });
    }
}

/// Paint `history` in `rect` as of `now`, its notes against A4 at
/// `reference` Hz.
pub fn paint(
    window: &mut gpui::Window,
    context: &mut gpui::App,
    rect: Rect,
    history: &History,
    (now, reference): (f64, f32),
    theme: &Theme,
) {
    let plot = Rect::new(
        rect.left + LABELS,
        rect.top + HEADER,
        rect.width - LABELS,
        rect.height - HEADER,
    );
    let (low, high) = history.lanes.unwrap_or((START - 6., START + 6.));
    let (bottom, top) = (low - 0.5, high + 0.5);
    let lane_height = plot.height / (top - bottom);
    let y = |lane: f32| plot.bottom() - (lane - bottom) * lane_height;
    let x = |time: f64| plot.right() - ((now - time) / SPAN) as f32 * plot.width;
    let current = history.points.back().and_then(|&(_, hz)| hz);
    let current_lane = current.map(|hz| midi(hz, reference).round() as i32);
    let current_color = match current {
        Some(hz) if note(hz, reference).in_tune() => theme.success,
        _ => theme.accent,
    };

    let shown = bottom.floor() as i32..=top.ceil() as i32;
    window.with_content_mask(
        Some(ContentMask {
            bounds: plot.bounds(),
        }),
        |window| {
            for lane in shown.clone() {
                let name = NAMES[lane.rem_euclid(12) as usize];
                let centre = y(lane as f32);
                let band = Rect::new(
                    plot.left,
                    centre - lane_height / 2.,
                    plot.width,
                    lane_height,
                );
                if current_lane == Some(lane) {
                    fill(window, band, faded(current_color, 0.12));
                } else if name.ends_with('#') {
                    fill(window, band, faded(theme.text, 0.03));
                }
                // Octaves apart under each C.
                if name == "C" {
                    fill(
                        window,
                        Rect::new(plot.left, band.bottom().round(), plot.width, 1.),
                        theme.grid_bar,
                    );
                }
            }
        },
    );

    // Names, the most useful first, each where it doesn't cover another:
    // the note now, the notes played, the Cs, then the rest if roomy.
    let since = now - SPAN;
    let played: Vec<i32> = history
        .points
        .iter()
        .filter(|&&(time, _)| time >= since)
        .filter_map(|&(_, hz)| hz)
        .map(|hz| midi(hz, reference).round() as i32)
        .collect();
    // With their octave, but for the rest.
    let mut order: Vec<(i32, Color, bool)> = current_lane
        .map(|lane| (lane, current_color, true))
        .into_iter()
        .chain(played.iter().map(|&lane| (lane, theme.text_muted, true)))
        .chain(
            shown
                .clone()
                .filter(|lane| lane.rem_euclid(12) == 0)
                .map(|lane| (lane, theme.text_muted, true)),
        )
        .chain(shown.map(|lane| (lane, theme.text_disabled, false)))
        .collect();
    if lane_height < NAMED_LANE {
        order.retain(|&(_, _, octave)| octave);
    }
    let labels = Rect::new(rect.left, plot.top, LABELS, plot.height);
    window.with_content_mask(
        Some(ContentMask {
            bounds: labels.bounds(),
        }),
        |window| {
            let mut named: Vec<f32> = Vec::new();
            for (lane, color, octave) in order {
                let centre = y(lane as f32);
                if named
                    .iter()
                    .any(|&other| (other - centre).abs() < NAMED_LANE)
                {
                    continue;
                }
                named.push(centre);
                let name = NAMES[lane.rem_euclid(12) as usize];
                let label = if octave {
                    format!("{name}{}", lane.div_euclid(12) - 1)
                } else {
                    name.to_owned()
                };
                text(
                    window,
                    context,
                    label,
                    (rect.left + 2., centre),
                    Align(0., 0.5),
                    size::SMALL,
                    color,
                    None,
                );
            }
        },
    );

    // The trace, a line per run of one colour, broken where nothing was
    // heard and across leaps.
    window.with_content_mask(
        Some(ContentMask {
            bounds: plot.bounds(),
        }),
        |window| {
            let mut run: Vec<(f32, f32)> = Vec::new();
            let mut run_color = theme.accent;
            let mut previous: Option<f32> = None;
            for &(time, heard) in &history.points {
                let point = heard.map(|hz| (midi(hz, reference), note(hz, reference).in_tune()));
                let joined = matches!(
                    (previous, point),
                    (Some(last), Some((lane, _))) if (lane - last).abs() < 0.5
                );
                if !joined {
                    flush(window, &mut run, run_color);
                }
                let Some((lane, in_tune)) = point else {
                    previous = None;
                    continue;
                };
                let color = if in_tune { theme.success } else { theme.accent };
                let at = (x(time), y(lane));
                if color != run_color && !run.is_empty() {
                    // The new colour carries on from the last point.
                    let last = run[run.len() - 1];
                    flush(window, &mut run, run_color);
                    run.push(last);
                }
                run_color = color;
                run.push(at);
                previous = Some(lane);
            }
            flush(window, &mut run, run_color);
        },
    );

    // The note now, at the top right.
    let (label, color) = match current.map(|hz| note(hz, reference)) {
        Some(heard) => (
            format!("{}{}  {:+.0} ct", heard.name, heard.octave, heard.cents),
            if heard.in_tune() {
                theme.success
            } else {
                theme.text
            },
        ),
        None => ("No pitch".into(), theme.text_disabled),
    };
    text(
        window,
        context,
        label,
        (rect.right() - 2., rect.top + HEADER / 2. - 1.),
        Align(1., 0.5),
        size::BODY,
        color,
        None,
    );
}

/// Draw `run` as one line, and empty it.
fn flush(window: &mut gpui::Window, run: &mut Vec<(f32, f32)>, color: Color) {
    if run.len() > 1 {
        polyline(window, run, 2., color);
    }
    run.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_what_is_in_view_and_one_before() {
        let mut history = History::default();
        for step in 0..=100 {
            history.push(step as f64 * 0.1, Some(440.));
        }
        let first = history
            .points
            .front()
            .map(|&(time, _)| time)
            .unwrap_or_default();
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
        history.push(0.1, Some(82.41));
        history.follow(440., 10.);
        let (low, high) = history.lanes.unwrap_or_default();
        assert!((low - 38.).abs() < 0.01 && (high - 71.).abs() < 0.01);
        history.push(20., Some(440.));
        history.push(20.1, Some(440.));
        history.follow(440., 0.01);
        let (low, _) = history.lanes.unwrap_or_default();
        assert!(low > 38. && low < 45., "{low}");
        let before = history.lanes;
        history.push(40., None);
        history.push(40.1, None);
        history.follow(440., 1.);
        assert_eq!(history.lanes, before);
    }
}
