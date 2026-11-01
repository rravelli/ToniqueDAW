use std::time::Duration;

use egui::{Rect, Vec2};
use serde::{Deserialize, Serialize};
use tonique_engine::time::{BeatPos, TimeSignature};

/// How close (in points) edits snap to clip edges, loop edges and the edit
/// cursor. They win over the grid when in reach.
pub const TARGET_REACH: f32 = 8.;
/// Closest the lines of a fixed grid are drawn: zoomed out further, coarser
/// lines are shown (snapping still goes by the chosen step).
const MIN_LINE_SPACING: f32 = 6.;
/// Positions are counted in these fractions of a quarter note: bars, beats
/// and every step (thirty-second triplets included) are whole numbers of
/// them, in any signature.
const TICKS: f64 = 96.;
/// Bars the coarsest lines span.
const MAX_BARS: u32 = 256;
/// Estimated width of a digit or dot of a ruler label, and the room left
/// between two labels.
pub const LABEL_CHAR_WIDTH: f32 = 6.;
pub const LABEL_GAP: f32 = 10.;
/// Steps of the time labels, in seconds.
const TIME_STEPS: [f64; 21] = [
    0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1., 2., 5., 10., 15., 30., 60., 120.,
    300., 600., 1800., 3600.,
];
const EPSILON: f64 = 1e-6;

/// A step the grid can be fixed to, from the coarsest to the finest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GridStep {
    EightBars,
    FourBars,
    TwoBars,
    Bar,
    Half,
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
}

impl GridStep {
    pub const ALL: [GridStep; 9] = [
        GridStep::EightBars,
        GridStep::FourBars,
        GridStep::TwoBars,
        GridStep::Bar,
        GridStep::Half,
        GridStep::Quarter,
        GridStep::Eighth,
        GridStep::Sixteenth,
        GridStep::ThirtySecond,
    ];

    /// Length in quarter-note beats; notes `triplet` last two thirds as
    /// long.
    pub fn beats(self, signature: TimeSignature, triplet: bool) -> f64 {
        let bars = |n: f64| n * signature.beats_per_bar();
        let note = |n: f64| if triplet { n * 2. / 3. } else { n };
        match self {
            GridStep::EightBars => bars(8.),
            GridStep::FourBars => bars(4.),
            GridStep::TwoBars => bars(2.),
            GridStep::Bar => bars(1.),
            GridStep::Half => note(2.),
            GridStep::Quarter => note(1.),
            GridStep::Eighth => note(0.5),
            GridStep::Sixteenth => note(0.25),
            GridStep::ThirtySecond => note(0.125),
        }
    }

    /// A note value (which can be a triplet), not a number of bars.
    pub fn is_note(self) -> bool {
        self >= GridStep::Half
    }

    pub fn finer(self) -> Self {
        Self::ALL[(self as usize + 1).min(Self::ALL.len() - 1)]
    }

    pub fn coarser(self) -> Self {
        Self::ALL[(self as usize).saturating_sub(1)]
    }

    /// `1/16`, `1/8T`, `2 bars`.
    pub fn name(self, triplet: bool) -> String {
        let name = match self {
            GridStep::EightBars => "8 bars",
            GridStep::FourBars => "4 bars",
            GridStep::TwoBars => "2 bars",
            GridStep::Bar => "1 bar",
            GridStep::Half => "1/2",
            GridStep::Quarter => "1/4",
            GridStep::Eighth => "1/8",
            GridStep::Sixteenth => "1/16",
            GridStep::ThirtySecond => "1/32",
        };
        if triplet && self.is_note() {
            format!("{name}T")
        } else {
            name.into()
        }
    }
}

/// How close the lines of an adaptive grid are, on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GridDensity {
    Wider,
    Medium,
    Narrower,
}

impl GridDensity {
    pub const ALL: [GridDensity; 3] = [
        GridDensity::Wider,
        GridDensity::Medium,
        GridDensity::Narrower,
    ];

    /// Closest two lines are drawn, in points.
    fn spacing(self) -> f32 {
        match self {
            GridDensity::Wider => 40.,
            GridDensity::Medium => 20.,
            GridDensity::Narrower => 10.,
        }
    }

    pub fn wider(self) -> Self {
        match self {
            GridDensity::Narrower => GridDensity::Medium,
            _ => GridDensity::Wider,
        }
    }

    pub fn narrower(self) -> Self {
        match self {
            GridDensity::Wider => GridDensity::Medium,
            _ => GridDensity::Narrower,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            GridDensity::Wider => "Wider",
            GridDensity::Medium => "Medium",
            GridDensity::Narrower => "Narrower",
        }
    }
}

/// What the grid shows and snaps to, saved with the project.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GridSettings {
    /// Lines follow the zoom, as close as `density` says; otherwise they
    /// stay `step` apart.
    pub adaptive: bool,
    pub density: GridDensity,
    pub step: GridStep,
    /// Note values are triplets.
    pub triplet: bool,
    /// Edits snap to the grid (Alt inverts it while held).
    pub snap: bool,
}

impl Default for GridSettings {
    fn default() -> Self {
        Self {
            adaptive: true,
            density: GridDensity::Medium,
            step: GridStep::Sixteenth,
            triplet: false,
            snap: true,
        }
    }
}

/// What a grid line marks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GridLine {
    /// The start of a bar (numbered from 1).
    Bar(i64),
    /// A felt beat inside a bar (both numbered from 1): the signature's
    /// beats, or in compound meters (6/8, 12/8…) groups of three.
    Beat { bar: i64, beat: u32 },
    /// A subdivision of a beat; `sub` counts the lines' steps from the
    /// beat (which is 1), when they divide it evenly.
    Division {
        bar: i64,
        beat: u32,
        sub: Option<u32>,
    },
}

pub struct GridService {
    pixels_per_beat: f32,
    /// A copy of the edit's, kept in sync by the project state.
    time_signature: TimeSignature,
    pub settings: GridSettings,
    min_pixels_per_beat: f32,
    max_pixels_per_beat: f32,
    pub offset: Vec2,
    /// Whether the view may scroll before beat 0, with grid lines and
    /// labels there too (bars count down from -1).
    negative_offset: bool,
}

/// Whether `step` fits a whole number of times in `length`.
fn divides(length: f64, step: f64) -> bool {
    let times = length / step;
    (times - times.round()).abs() < EPSILON
}

fn ticks(beats: f64) -> i64 {
    (beats * TICKS).round() as i64
}

/// Characters of `n` written out.
fn digits(n: i64) -> u32 {
    n.unsigned_abs().max(1).ilog10() + 1 + u32::from(n < 0)
}

impl GridService {
    pub fn new() -> Self {
        Self {
            pixels_per_beat: 10.,
            time_signature: TimeSignature::default(),
            settings: GridSettings::default(),
            min_pixels_per_beat: 0.5,
            max_pixels_per_beat: 5000.,
            offset: Vec2::ZERO,
            negative_offset: false,
        }
    }
    pub fn set_time_signature(&mut self, signature: TimeSignature) {
        self.time_signature = signature;
    }
    /// Length of a bar, in quarter-note beats.
    pub fn beats_per_bar(&self) -> f64 {
        self.time_signature.beats_per_bar()
    }
    /// Length of the felt beat, in quarter-note beats: the signature's
    /// beat, or three of them in compound meters (a dotted quarter in 6/8).
    pub fn pulse(&self) -> f64 {
        let signature = self.time_signature;
        let compound = signature.numerator > 3
            && signature.numerator.is_multiple_of(3)
            && signature.denominator >= 8;
        let beat = signature.beat_length();
        if compound { 3. * beat } else { beat }
    }

    /// The steps an adaptive grid picks from, finest first: note values
    /// dividing the beat, the beat, then 1, 2, 4… bars. Each one's lines
    /// fall on the next one's.
    fn ladder(&self) -> Vec<f64> {
        let (pulse, bar) = (self.pulse(), self.beats_per_bar());
        let triplet = self.settings.triplet;
        let mut steps: Vec<f64> = [0.125, 0.25, 0.5, 1., 2.]
            .map(|n| if triplet { n * 2. / 3. } else { n })
            .into_iter()
            .filter(|step| *step < pulse - EPSILON && divides(pulse, *step))
            .collect();
        if pulse < bar - EPSILON {
            steps.push(pulse);
        }
        steps.extend((0..=MAX_BARS.ilog2()).map(|n| bar * f64::from(1 << n)));
        steps
    }

    /// `step`, or the first coarser step of the ladder whose lines fall on
    /// its own (or on bars) that `fits`; the coarsest when none does.
    fn first_fitting(&self, step: f64, fits: impl Fn(f64) -> bool) -> f64 {
        let bar = self.beats_per_bar();
        std::iter::once(step)
            .chain(
                self.ladder()
                    .into_iter()
                    .filter(|s| *s > step + EPSILON && (*s >= bar - EPSILON || divides(*s, step))),
            )
            .find(|s| fits(*s))
            .unwrap_or(bar * f64::from(MAX_BARS))
    }

    /// The fixed step, in beats.
    fn fixed_step(&self) -> f64 {
        self.settings
            .step
            .beats(self.time_signature, self.settings.triplet)
    }

    /// Spacing between the grid lines drawn at this zoom, in beats.
    pub fn line_step(&self) -> f64 {
        let (step, spacing) = if self.settings.adaptive {
            (self.ladder()[0], self.settings.density.spacing())
        } else {
            (self.fixed_step(), MIN_LINE_SPACING)
        };
        self.first_fitting(step, |s| self.beats_to_width(s) >= spacing)
    }

    /// Spacing between the positions edits snap to, in beats: the lines
    /// drawn, or the fixed step however far zoomed out.
    pub fn snap_step(&self) -> f64 {
        if self.settings.adaptive {
            self.line_step()
        } else {
            self.fixed_step()
        }
    }

    /// `step` as a menu shows it: `1/16`, `1/8T`, `3/8`, `2 bars`.
    pub fn step_name(&self, step: f64) -> String {
        let bar = self.beats_per_bar();
        if step >= bar - EPSILON {
            return match (step / bar).round() as u32 {
                1 => "1 bar".into(),
                n => format!("{n} bars"),
            };
        }
        // As a fraction of a whole note.
        let whole = step / 4.;
        let denominator = (1..=4 * TICKS as u32)
            .find(|q| divides(whole * f64::from(*q), 1.))
            .unwrap_or(1);
        let numerator = (whole * f64::from(denominator)).round() as u32;
        if numerator == 1 && denominator % 3 == 0 {
            format!("1/{}T", denominator * 2 / 3)
        } else {
            format!("{numerator}/{denominator}")
        }
    }

    // Changing the settings, as the shortcuts do.

    /// Closer lines: a finer fixed step, or a narrower adaptive grid.
    pub fn narrow(&mut self) {
        let settings = &mut self.settings;
        if settings.adaptive {
            settings.density = settings.density.narrower();
        } else {
            settings.step = settings.step.finer();
        }
    }

    /// Lines further apart: a coarser fixed step, or a wider adaptive grid.
    pub fn widen(&mut self) {
        let settings = &mut self.settings;
        if settings.adaptive {
            settings.density = settings.density.wider();
        } else {
            settings.step = settings.step.coarser();
        }
    }

    /// Switch between adaptive and fixed. A grid becoming fixed keeps the
    /// step it shows (or the nearest there is).
    pub fn toggle_adaptive(&mut self) {
        if self.settings.adaptive {
            let shown = self.line_step();
            let (signature, triplet) = (self.time_signature, self.settings.triplet);
            self.settings.step = GridStep::ALL
                .into_iter()
                .min_by(|a, b| {
                    let off = |s: GridStep| (s.beats(signature, triplet) / shown).ln().abs();
                    off(*a).total_cmp(&off(*b))
                })
                .unwrap_or(GridStep::Sixteenth);
        }
        self.settings.adaptive = !self.settings.adaptive;
    }

    /// The grid lines `step` apart from `from` to `to` (in beats), and
    /// what each marks. Steps shorter than a bar start over at each bar
    /// (a half-note grid in 7/8 has a line at the bar and a half note
    /// in); longer ones count whole bars from the start.
    pub fn lines(&self, from: BeatPos, to: BeatPos, step: f64) -> Vec<(BeatPos, GridLine)> {
        let bar = ticks(self.beats_per_bar()).max(1);
        let step = ticks(step).max(1);
        let (from, to) = (
            (from.0 * TICKS).floor() as i64,
            (to.0 * TICKS).ceil() as i64,
        );
        let mut lines = Vec::new();
        let mut push = |tick: i64| {
            if (from..=to).contains(&tick) {
                lines.push((BeatPos(tick as f64 / TICKS), self.line_at(tick, step)));
            }
        };
        if step >= bar {
            let span = bar * (step as f64 / bar as f64).round().max(1.) as i64;
            for n in from.div_euclid(span)..=to.div_euclid(span) {
                push(n * span);
            }
        } else {
            for n in from.div_euclid(bar)..=to.div_euclid(bar) {
                for tick in (n * bar..(n + 1) * bar).step_by(step as usize) {
                    push(tick);
                }
            }
        }
        lines
    }

    /// What the line at `tick` of a grid `step` ticks apart marks.
    fn line_at(&self, tick: i64, step: i64) -> GridLine {
        let bar = ticks(self.beats_per_bar()).max(1);
        let pulse = ticks(self.pulse()).max(1);
        let in_bar = tick.rem_euclid(bar);
        // From 1, and before beat 0 from -1 down: there's no bar 0.
        let number = match tick.div_euclid(bar) {
            n if n >= 0 => n + 1,
            n => n,
        };
        let beat = (in_bar / pulse) as u32 + 1;
        let in_beat = in_bar % pulse;
        if in_bar == 0 {
            GridLine::Bar(number)
        } else if in_beat == 0 {
            GridLine::Beat { bar: number, beat }
        } else {
            GridLine::Division {
                bar: number,
                beat,
                sub: (in_beat % step == 0).then(|| (in_beat / step) as u32 + 1),
            }
        }
    }

    /// The beats `rect` shows, none before 0 unless the view may go there.
    pub fn visible_beats(&self, rect: Rect) -> (BeatPos, BeatPos) {
        let from = self.x_to_beats(rect.left(), rect);
        let from = if self.negative_offset {
            from
        } else {
            from.max(BeatPos::ZERO)
        };
        (from, self.x_to_beats(rect.right(), rect))
    }

    /// Spacing between labelled lines from `from` to `to`, in beats: the
    /// finest step, from the lines drawn up, whose labels fit between
    /// them.
    pub fn label_step(&self, from: BeatPos, to: BeatPos) -> f64 {
        let bar = |beats: BeatPos| {
            let n = (beats.0 / self.beats_per_bar()).floor() as i64;
            if n >= 0 { n + 1 } else { n }
        };
        let widest = if digits(bar(from)) > digits(bar(to)) {
            bar(from)
        } else {
            bar(to)
        };
        self.first_fitting(self.line_step(), |s| {
            self.beats_to_width(s) >= self.label_width(s, widest)
        })
    }

    /// Estimated room the labels of lines `step` apart take, bar numbers
    /// as long as `bar`'s: `12`, `12.3`, `12.3.4`.
    pub fn label_width(&self, step: f64, bar: i64) -> f32 {
        let (pulse, bar_length) = (self.pulse(), self.beats_per_bar());
        let count = |length: f64, step: f64| (length / step - EPSILON).ceil() as i64;
        let mut chars = digits(bar);
        if step < bar_length - EPSILON {
            chars += 1 + digits(count(bar_length, pulse));
        }
        if step < pulse - EPSILON {
            chars += 1 + digits(count(pulse, step));
        }
        chars as f32 * LABEL_CHAR_WIDTH + LABEL_GAP
    }

    /// Seconds between time labels at `bpm`: the finest of 1, 2, 5, 10, 15,
    /// 30 s… (and fractions down to a millisecond) whose labels fit.
    pub fn time_step(&self, bpm: f32) -> f64 {
        let pixels_per_second = self.pixels_per_beat as f64 * bpm as f64 / 60.;
        TIME_STEPS
            .into_iter()
            .find(|step| {
                // `0:05`, or `0:01.250`.
                let chars = if *step < 1. { 8. } else { 5. };
                step * pixels_per_second >= chars * LABEL_CHAR_WIDTH as f64 + LABEL_GAP as f64
            })
            .unwrap_or(TIME_STEPS[TIME_STEPS.len() - 1])
    }

    /// The nearest snap position, however far.
    pub fn snap_to_step(&self, beats: BeatPos) -> BeatPos {
        let step = self.snap_step();
        let bar = self.beats_per_bar();
        if step >= bar - EPSILON {
            let span = bar * (step / bar).round().max(1.);
            return BeatPos((beats.0 / span).round() * span);
        }
        // From the bar's start, the last line before the next bar being
        // as near as it is.
        let start = (beats.0 / bar).floor() * bar;
        let into = beats.0 - start;
        let before = (into / step).floor() * step;
        let after = (before + step).min(bar);
        BeatPos(
            start
                + if into - before <= after - into {
                    before
                } else {
                    after
                },
        )
    }

    /// Whether edits snap, Alt being held or not.
    pub fn snaps(&self, alt: bool) -> bool {
        self.settings.snap != alt
    }

    /// `beats` snapped to the grid if [`Self::snaps`].
    pub fn snap(&self, beats: BeatPos, alt: bool) -> BeatPos {
        if self.snaps(alt) {
            self.snap_to_step(beats)
        } else {
            beats
        }
    }

    pub fn pixels_per_beat(&self) -> f32 {
        self.pixels_per_beat
    }
    /// Convert a time duration to an actual screen width
    pub fn duration_to_width(&self, duration: Duration, bpm: f32) -> f32 {
        duration.as_secs_f32() / 60.0 * bpm * self.pixels_per_beat
    }
    /// Position in beats to actual screen x position
    pub fn beats_to_x(&self, beats: BeatPos, viewport: Rect) -> f32 {
        viewport.left() + beats.0 as f32 * self.pixels_per_beat - self.offset.x
    }
    /// Actual x position to beats position
    pub fn x_to_beats(&self, x: f32, viewport: Rect) -> BeatPos {
        BeatPos(((x + self.offset.x - viewport.left()) / self.pixels_per_beat) as f64)
    }
    /// A width on screen, in beats.
    pub fn width_to_beats(&self, width: f32) -> f64 {
        (width / self.pixels_per_beat) as f64
    }
    /// A length in beats, as a width on screen.
    pub fn beats_to_width(&self, beats: f64) -> f32 {
        beats as f32 * self.pixels_per_beat
    }

    /// Zoom from a scroll delta, keeping the beat under `cursor_x` in place.
    pub fn zoom_around(&mut self, delta: f32, cursor_x: f32, viewport: Rect) {
        self.zoom_by((1.0 - delta * 0.007).clamp(0., 2.0), cursor_x, viewport);
    }

    /// Multiply the zoom by `factor`, keeping the beat under `cursor_x` in place.
    pub fn zoom_by(&mut self, factor: f32, cursor_x: f32, viewport: Rect) {
        let old_ppb = self.pixels_per_beat;
        let old_offset_x = self.offset.x;

        // Beat that the cursor is pointing to (world position)
        let beat_under_cursor = (cursor_x + old_offset_x - viewport.left()) / old_ppb;

        // Apply zoom (clamped)
        self.pixels_per_beat = (self.pixels_per_beat * factor)
            .clamp(self.min_pixels_per_beat, self.max_pixels_per_beat);

        // New offset that keeps the same beat under the cursor
        let new_ppb = self.pixels_per_beat;
        let new_offset_x = beat_under_cursor * new_ppb - (cursor_x - viewport.left());

        self.offset.x = self.bound_offset(new_offset_x);
    }

    /// Zoom and scroll so `start..end` (in beats) spans `width` points.
    pub fn fit(&mut self, start: BeatPos, end: BeatPos, width: f32) {
        if end > start && width > 0. {
            self.pixels_per_beat = (width / (end - start) as f32)
                .clamp(self.min_pixels_per_beat, self.max_pixels_per_beat);
        }
        self.offset.x = self.bound_offset(self.beats_to_width(start.0));
    }

    /// Let the view scroll before beat 0, e.g. to show what's before a
    /// clip in a grid counted from its start. The grid and its labels go
    /// on there.
    pub fn with_negative_offset(mut self) -> Self {
        self.negative_offset = true;
        self
    }

    /// Whether the view may show beats before 0 (see
    /// [`Self::with_negative_offset`]).
    pub fn allows_negative(&self) -> bool {
        self.negative_offset
    }

    fn bound_offset(&self, offset: f32) -> f32 {
        if self.negative_offset {
            offset
        } else {
            offset.max(0.)
        }
    }

    /// Snap `beats` to the nearest of `targets` within [`TARGET_REACH`]
    /// points, or else to the grid. Returns the snapped position and
    /// whether it came from `targets`.
    pub fn snap_to_targets(&self, beats: BeatPos, targets: &[BeatPos]) -> (BeatPos, bool) {
        let reach = self.width_to_beats(TARGET_REACH);
        let target = targets
            .iter()
            .copied()
            .filter(|t| (*t - beats).abs() <= reach)
            .min_by(|a, b| (*a - beats).abs().total_cmp(&(*b - beats).abs()));
        match target {
            Some(t) => (t, true),
            None => (self.snap_to_step(beats), false),
        }
    }
}

#[cfg(test)]
mod tests;
