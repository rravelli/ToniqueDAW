//! Editors of the dynamics effects, as the egui app's: each shows the gain
//! it takes off as it plays. The compressor over its input and output
//! levels of the last few seconds, its threshold dragged up and down; the
//! gate beside its curve, output level against input level, with the
//! input's level on it; the limiter as a trace of the last seconds.

use std::{cell::Cell, collections::VecDeque, rc::Rc, time::Instant};

use gpui::{
    AnyElement, Bounds, Context, CursorStyle, MouseButton, MouseDownEvent, Pixels, Window, canvas,
    div, prelude::*, px,
};
use tonique_engine::{
    dsp::gain_to_db,
    edit::{Parameter, PluginKind},
    meter::{Level, PluginTap},
    nodes::GATE_SILENT_DB,
};

use crate::{
    app::Tonique,
    bottom::{
        paint::{Align, circle, fill, fill_rounded, polygon, polyline, text},
        rack::{
            RackDrag,
            editors::{Editor, EditorInput, KNOB_GAP},
            format_db, format_ratio, format_time_ms,
            graph::hline,
            knob::{KNOB_HEIGHT, KNOB_WIDTH, SettingKnob},
        },
    },
    core::{
        effect::{Effect, LIMITER_LOOKAHEAD, LIMITER_LOOKAHEADS},
        metrics::AudioMetrics,
    },
    geometry::Rect,
    theme::{Theme, faded, mix, size},
};

/// How fast a reduction shown falls back, in dB per second.
const FALL_DB_PER_S: f32 = 30.;
/// The deepest reduction a meter shows, in dB.
const METER_DB: f32 = 24.;
const METER_WIDTH: f32 = 8.;
/// Width of a gate's curve.
pub const CURVE_WIDTH: f32 = 110.;
/// Between rows of knobs.
const ROW_GAP: f32 = 4.;
/// Seconds of a compressor's history shown.
const HISTORY_S: f32 = 4.;
/// The bottom of a compressor's history, in dB.
const HISTORY_DB: f32 = -60.;
/// Levels marked on a compressor's history.
const HISTORY_MARKS: [f32; 3] = [0., -20., -40.];
/// Readings kept for a limiter's history, one a frame.
const LIMITER_HISTORY: usize = 240;

/// Gain taken off, as a level change.
pub fn format_reduction(db: f32) -> String {
    if db < 0.05 {
        "0.0 dB".into()
    } else {
        format!("-{db:.1} dB")
    }
}

/// A gate's range: silent at its bottom.
pub fn format_range(db: f32) -> String {
    if db <= GATE_SILENT_DB {
        "-inf dB".into()
    } else {
        format!("{db:.0} dB")
    }
}

/// Width of a gate's frame: its curve, its meter and three columns of
/// knobs.
pub fn gate_width() -> f32 {
    CURVE_WIDTH + METER_WIDTH + 2. * KNOB_GAP + 3. * KNOB_WIDTH + 2. * KNOB_GAP + 22.
}

/// The gain reduction shown: jumps to each new reading's, falls back
/// slowly so it can be read.
#[derive(Default)]
struct Reduction {
    shown: f32,
    last: Option<Instant>,
}

impl Reduction {
    fn update(&mut self, tap: &PluginTap) -> f32 {
        let now = Instant::now();
        let seconds = self
            .last
            .map_or(0., |last| (now - last).as_secs_f32().min(0.25));
        self.last = Some(now);
        self.shown = (self.shown - FALL_DB_PER_S * seconds).max(0.);
        if let Some(db) = tap.reduction.take() {
            self.shown = self.shown.max(db);
        }
        self.shown
    }
}

/// The levels of one stretch of audio through a compressor, in dB.
struct Reading {
    frames: u32,
    input: f32,
    output: f32,
    reduction: f32,
}

/// A compressor's last few seconds, newest last.
pub struct CompressorState {
    history: VecDeque<Reading>,
    /// The graph, as last laid out.
    graph: Rc<Cell<Bounds<Pixels>>>,
}

impl CompressorState {
    pub fn new() -> Self {
        Self {
            history: VecDeque::new(),
            graph: Rc::default(),
        }
    }

    /// Adds what was heard since the last frame (`output`, its levels out,
    /// taken for its meter), and drops what's out of sight.
    pub fn record(&mut self, effect: &Effect, sample_rate: f32, output: Option<[Level; 2]>) {
        let tap = &effect.plugin.tap;
        let (input, reduction) = (tap.input.take_levels(), tap.reduction.take());
        tap.watch_input_for((sample_rate / 2.) as u32);
        let Some(input) = input else {
            return;
        };
        let peak = |levels: [Level; 2]| gain_to_db(levels[0].peak.max(levels[1].peak));
        let last = self.history.back().map_or(0., |reading| reading.reduction);
        self.history.push_back(Reading {
            frames: input[0].frames,
            input: peak(input),
            output: output.map_or(HISTORY_DB, peak),
            reduction: reduction.unwrap_or(last),
        });
        let span = (HISTORY_S * sample_rate) as u32;
        let mut frames: u32 = self.history.iter().map(|reading| reading.frames).sum();
        while frames > span
            && let Some(old) = self.history.pop_front()
        {
            frames -= old.frames;
        }
    }
}

/// A gate's input level and the reduction to show.
pub struct GateState {
    reduction: Reduction,
    input: AudioMetrics,
    /// The input's peak now, in dBFS, and the reduction shown, in dB.
    shown: (Option<f32>, f32),
}

impl GateState {
    pub fn new() -> Self {
        Self {
            reduction: Reduction::default(),
            input: AudioMetrics::new(),
            shown: (None, 0.),
        }
    }

    pub fn update(&mut self, tap: &PluginTap, sample_rate: f32) {
        let reduction = self.reduction.update(tap);
        tap.watch_input_for((sample_rate / 2.) as u32);
        self.input.update(&tap.input, false);
        let [left, right] = self.input.peak_db();
        self.shown = (Some(left.max(right)), reduction);
    }
}

/// A limiter's reductions shown, newest last.
pub struct LimiterState {
    reduction: Reduction,
    history: VecDeque<f32>,
}

impl LimiterState {
    pub fn new() -> Self {
        Self {
            reduction: Reduction::default(),
            history: VecDeque::with_capacity(LIMITER_HISTORY),
        }
    }

    pub fn update(&mut self, tap: &PluginTap) {
        let reduction = self.reduction.update(tap);
        if self.history.len() == LIMITER_HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(reduction);
    }
}

/// A level line dragged up and down: a compressor's threshold.
pub struct LevelDrag {
    pub param: Parameter,
    pub before: f32,
    pub graph: Rc<Cell<Bounds<Pixels>>>,
}

impl LevelDrag {
    /// Follows the pointer to `y`.
    pub fn follow(&self, y: f32) {
        let rect = Rect::of(self.graph.get());
        if rect.is_empty() {
            return;
        }
        let db = (y - rect.top) / rect.height * HISTORY_DB;
        self.param.set(db.clamp(self.param.min, self.param.max));
    }
}

/// Output level against input level, down to `bottom_db` each way.
struct Curve {
    rect: Rect,
    bottom_db: f32,
}

impl Curve {
    fn at(&self, input: f32, output: f32) -> (f32, f32) {
        let along = |db: f32| (1. - db / self.bottom_db).clamp(0., 1.);
        (
            self.rect.left + along(input) * self.rect.width,
            self.rect.bottom() - along(output) * self.rect.height,
        )
    }

    /// The grid, the unity line, `out` for each input level, and a dot
    /// where the input's level is now.
    fn paint(
        &self,
        window: &mut Window,
        theme: &Theme,
        color: crate::theme::Color,
        level: Option<f32>,
        out: impl Fn(f32) -> f32,
    ) {
        let rect = self.rect;
        fill_rounded(window, rect, 2., theme.bg_deep);
        let mut db = -12.;
        while db > self.bottom_db {
            let (x, y) = self.at(db, db);
            fill(
                window,
                Rect::new(x.round(), rect.top, 1., rect.height),
                theme.grid_beat,
            );
            hline(window, rect, y, theme.grid_beat);
            db -= 12.;
        }
        polyline(
            window,
            &[self.at(self.bottom_db, self.bottom_db), self.at(0., 0.)],
            1.,
            theme.grid_bar,
        );
        let steps = rect.width.ceil() as usize;
        let points: Vec<(f32, f32)> = (0..=steps)
            .map(|step| {
                let input = self.bottom_db * (1. - step as f32 / steps as f32);
                self.at(input, out(input))
            })
            .collect();
        polyline(window, &points, 1.5, color);
        if let Some(level) = level.filter(|level| *level > self.bottom_db) {
            circle(window, self.at(level, out(level)), 3., theme.text, None);
        }
    }
}

/// A bar down from the top of `rect`, as deep as `db` is.
fn paint_reduction_meter(window: &mut Window, rect: Rect, db: f32, theme: &Theme) {
    fill_rounded(window, rect, 2., theme.bg_deep);
    let y = |db: f32| rect.top + (db / METER_DB).clamp(0., 1.) * rect.height;
    if db > 0.01 {
        fill_rounded(
            window,
            Rect::new(rect.left, rect.top, rect.width, y(db) - rect.top),
            2.,
            theme.warning,
        );
    }
    for mark in [6., 12., 18.] {
        hline(window, rect, y(mark), theme.grid_beat);
    }
}

fn accent(theme: &Theme, enabled: bool) -> crate::theme::Color {
    if enabled {
        theme.accent
    } else {
        theme.text_disabled
    }
}

impl Tonique {
    /// The input and output levels over the last few seconds, newest at
    /// the right, with the gain taken off down from the top and the
    /// threshold, dragged anywhere; then its knobs.
    pub(super) fn compressor_body(
        &self,
        input: &EditorInput,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let effect = input.effect;
        let enabled = input.enabled();
        let sample_rate = input.sample_rate;
        let threshold = effect.plugin.param("threshold").cloned();
        let initial = effect.kind.initial_value(&effect.plugin, "threshold");
        let graph_bounds = match &*input.editor.borrow() {
            Editor::Compressor(compressor) => compressor.graph.clone(),
            _ => return div().into_any_element(),
        };
        let editor = input.editor.clone();
        let paint_theme = theme.clone();
        let paint_threshold = threshold.as_ref().map(|threshold| threshold.get());
        let slot = graph_bounds.clone();
        let painting = canvas(
            move |bounds, _, _| slot.set(bounds),
            move |bounds, _, window, context| {
                let theme = &paint_theme;
                let rect = Rect::of(bounds);
                let y = |db: f32| rect.top + (db / HISTORY_DB).clamp(0., 1.) * rect.height;
                fill_rounded(window, rect, 2., theme.bg_deep);
                for mark in HISTORY_MARKS {
                    hline(window, rect, y(mark), theme.grid_beat);
                    text(
                        window,
                        context,
                        format!("{mark:.0} dB"),
                        (rect.right() - 3., y(mark) + 2.),
                        Align::RIGHT_TOP,
                        size::SMALL,
                        theme.text_muted,
                        None,
                    );
                }
                let editor = editor.borrow();
                let Editor::Compressor(compressor) = &*editor else {
                    return;
                };
                // Each reading is a column as wide as the audio it's over.
                let (input_color, output_color, reduction_color) = if enabled {
                    (
                        mix(theme.bg_deep, theme.text_muted, 0.6),
                        mix(theme.bg_deep, theme.text_muted, 0.3),
                        theme.warning,
                    )
                } else {
                    (
                        mix(theme.bg_deep, theme.text_disabled, 0.4),
                        mix(theme.bg_deep, theme.text_disabled, 0.2),
                        theme.text_disabled,
                    )
                };
                let per_frame = rect.width / (HISTORY_S * sample_rate);
                let mut right = rect.right();
                let mut reduction = Vec::with_capacity(compressor.history.len() * 2);
                for reading in compressor.history.iter().rev() {
                    let left = right - reading.frames as f32 * per_frame;
                    for (db, color) in
                        [(reading.input, input_color), (reading.output, output_color)]
                    {
                        let top = y(db);
                        fill(
                            window,
                            Rect::new(left, top, right - left, rect.bottom() - top),
                            color,
                        );
                    }
                    let at = y(-reading.reduction);
                    reduction.extend([(right, at), (left, at)]);
                    right = left;
                    if right < rect.left {
                        break;
                    }
                }
                polyline(window, &reduction, 1.5, reduction_color);
                if let Some(threshold) = paint_threshold {
                    let at = y(threshold);
                    let color = if enabled {
                        theme.text
                    } else {
                        theme.text_disabled
                    };
                    fill(
                        window,
                        Rect::new(rect.left, at - 0.75, rect.width, 1.5),
                        color,
                    );
                    text(
                        window,
                        context,
                        format!("Threshold {}", format_db(threshold)),
                        (rect.left + 4., at - 2.),
                        Align::LEFT_BOTTOM,
                        size::SMALL,
                        color,
                        None,
                    );
                }
                let shown = compressor
                    .history
                    .back()
                    .map_or(0., |reading| reading.reduction);
                text(
                    window,
                    context,
                    format!("GR {}", format_reduction(shown)),
                    (rect.left + 4., rect.top + 3.),
                    Align::LEFT_TOP,
                    size::SMALL,
                    theme.text,
                    None,
                );
            },
        )
        .absolute()
        .size_full();
        let graph = div()
            .id("compressor-graph")
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .cursor(CursorStyle::ResizeUpDown)
            .child(painting)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, event: &MouseDownEvent, _, context| {
                    context.stop_propagation();
                    let Some(threshold) = threshold.clone() else {
                        return;
                    };
                    let before = threshold.get();
                    this.bottom.rack.drag = None;
                    if event.click_count >= 2 {
                        if let Some(value) = initial {
                            threshold.set(value);
                            this.state
                                .commit_params(&[(threshold.id, before, threshold.get())]);
                        }
                    } else {
                        let drag = LevelDrag {
                            param: threshold,
                            before,
                            graph: graph_bounds.clone(),
                        };
                        this.bottom.rack.drag = Some(RackDrag::Level(drag));
                    }
                    context.notify();
                }),
            );
        let knobs = self.knob_row(
            input,
            &[
                ("attack", "Attack", format_time_ms, true),
                ("release", "Release", format_time_ms, true),
                ("ratio", "Ratio", format_ratio, true),
                ("threshold", "Threshold", format_db, false),
                ("knee", "Knee", format_db, false),
                ("makeup", "Makeup", format_db, false),
            ],
            [],
            theme,
            context,
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(graph)
            .child(knobs)
            .into_any_element()
    }

    /// Its curve with the input's level on it, the reduction's meter, and
    /// its knobs in rows of three.
    pub(super) fn gate_body(
        &self,
        input: &EditorInput,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let effect = input.effect;
        let value = |name| effect.plugin.param(name).map_or(0., |param| param.get());
        let (threshold, range) = (value("threshold"), value("range"));
        let enabled = input.enabled();
        let (level, reduction) = match &*input.editor.borrow() {
            Editor::Gate(gate) if enabled => gate.shown,
            _ => (None, 0.),
        };
        let paint_theme = theme.clone();
        let curve = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let theme = &paint_theme;
                let bottom_db = GATE_SILENT_DB;
                Curve {
                    rect: Rect::of(bounds),
                    bottom_db,
                }
                .paint(window, theme, accent(theme, enabled), level, |input| {
                    if input >= threshold {
                        input
                    } else if range <= GATE_SILENT_DB {
                        bottom_db
                    } else {
                        input + range
                    }
                });
            },
        )
        .flex_none()
        .w(px(CURVE_WIDTH))
        .h_full();
        let meter_theme = theme.clone();
        let meter = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                paint_reduction_meter(window, Rect::of(bounds), reduction, &meter_theme);
            },
        )
        .flex_none()
        .w(px(METER_WIDTH))
        .h_full();
        let top = self.knob_row(
            input,
            &[
                ("threshold", "Threshold", format_db, false),
                ("range", "Range", format_range, false),
                ("hold", "Hold", format_time_ms, false),
            ],
            [],
            theme,
            context,
        );
        let bottom = self.knob_row(
            input,
            &[
                ("attack", "Attack", format_time_ms, true),
                ("release", "Release", format_time_ms, true),
            ],
            [],
            theme,
            context,
        );
        div()
            .size_full()
            .flex()
            .gap(px(KNOB_GAP))
            .child(curve)
            .child(meter)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(ROW_GAP))
                    .child(top)
                    .child(bottom),
            )
            .into_any_element()
    }

    /// The reduction over the last few seconds, down from the top, newest
    /// at the right; then its knobs.
    pub(super) fn limiter_body(
        &self,
        input: &EditorInput,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let PluginKind::Limiter { lookahead_ms } = input.effect.plugin.kind else {
            return div().into_any_element();
        };
        let enabled = input.enabled();
        let editor = input.editor.clone();
        let paint_theme = theme.clone();
        let graph = canvas(
            |_, _, _| {},
            move |bounds, _, window, context| {
                let theme = &paint_theme;
                let rect = Rect::of(bounds);
                fill_rounded(window, rect, 2., theme.bg_deep);
                let y = |db: f32| rect.top + (db / METER_DB).clamp(0., 1.) * rect.height;
                for mark in [6., 12., 18.] {
                    hline(window, rect, y(mark), theme.grid_beat);
                    text(
                        window,
                        context,
                        format!("-{mark:.0}"),
                        (rect.left + 2., y(mark) - 1.),
                        Align::LEFT_BOTTOM,
                        size::SMALL,
                        theme.text_muted,
                        None,
                    );
                }
                let editor = editor.borrow();
                let Editor::Limiter(limiter) = &*editor else {
                    return;
                };
                let step = rect.width / (LIMITER_HISTORY - 1) as f32;
                let start = rect.right() - (limiter.history.len() as f32 - 1.) * step;
                let points: Vec<(f32, f32)> = limiter
                    .history
                    .iter()
                    .enumerate()
                    .map(|(index, db)| {
                        let db = if enabled { *db } else { 0. };
                        (start + index as f32 * step, y(db))
                    })
                    .collect();
                let warm = if enabled {
                    theme.warning
                } else {
                    theme.text_disabled
                };
                for pair in points.windows(2) {
                    polygon(
                        window,
                        &[
                            (pair[0].0, rect.top),
                            (pair[1].0, rect.top),
                            pair[1],
                            pair[0],
                        ],
                        faded(warm, 0.3),
                    );
                }
                polyline(window, &points, 1.5, warm);
                let now = match limiter.history.back() {
                    Some(db) if enabled => *db,
                    _ => 0.,
                };
                text(
                    window,
                    context,
                    format_reduction(now),
                    (rect.right() - 4., rect.top + 3.),
                    Align::RIGHT_TOP,
                    size::SMALL,
                    theme.text,
                    None,
                );
            },
        )
        .flex_1()
        .min_h_0()
        .w_full();
        let lookahead = self.setting_knob(
            input,
            SettingKnob::Lookahead,
            (lookahead_ms, LIMITER_LOOKAHEADS, LIMITER_LOOKAHEAD),
            "Ahead",
            format_time_ms,
            true,
            theme,
            context,
        );
        let knobs = self
            .knob_row(
                input,
                &[
                    ("gain", "Gain", format_db, false),
                    ("ceiling", "Ceiling", format_db, false),
                    ("release", "Release", format_time_ms, true),
                ],
                [lookahead.into_any_element()],
                theme,
                context,
            )
            .h(px(KNOB_HEIGHT));
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(graph)
            .child(knobs)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_well() {
        assert_eq!(format_range(GATE_SILENT_DB), "-inf dB");
        assert_eq!(format_range(-20.), "-20 dB");
        assert_eq!(format_reduction(0.), "0.0 dB");
        assert_eq!(format_reduction(3.21), "-3.2 dB");
    }

    #[test]
    fn a_curve_spans_its_rect() {
        let curve = Curve {
            rect: Rect::new(0., 0., 100., 50.),
            bottom_db: -60.,
        };
        assert_eq!(curve.at(0., 0.), (100., 0.));
        assert_eq!(curve.at(-60., -60.), (0., 50.));
    }
}
