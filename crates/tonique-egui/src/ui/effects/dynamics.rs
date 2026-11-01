//! Editors of the dynamics effects:
//! [`Compressor`](crate::core::effect::EffectKind::Compressor),
//! [`Limiter`](crate::core::effect::EffectKind::Limiter) and
//! [`Gate`](crate::core::effect::EffectKind::Gate). Each shows the gain it
//! takes off as it plays: the compressor over its input and output levels
//! of the last few seconds; the gate beside its curve, output level against
//! input level, with the input's level on it.

use crate::ui::theme::font_size;
use crate::{
    core::{
        effect::{LIMITER_LOOKAHEAD, LIMITER_LOOKAHEADS, Setting},
        metrics::AudioMetrics,
    },
    ui::{
        effects::{EditorContext, EffectEdit, EffectEditor, format_db, graph_rect, track_drag},
        theme::{Theme, ThemeExt},
        widget::knob::Knob,
    },
};
use egui::{
    Align, Align2, Color32, CursorIcon, FontId, Layout, Painter, Pos2, Rect, Sense, Stroke, Ui,
    UiBuilder, pos2, vec2,
};
use std::collections::VecDeque;
use tonique_engine::{
    dsp::gain_to_db,
    edit::PluginKind,
    meter::{Level, PluginTap},
    nodes::GATE_SILENT_DB,
};

/// How fast a reduction shown falls back, in dB per second.
const FALL_DB_PER_S: f32 = 30.;
/// The deepest reduction a meter shows, in dB.
const METER_DB: f32 = 24.;
const METER_WIDTH: f32 = 8.;
/// Width of a gate's curve.
const CURVE_WIDTH: f32 = 110.;
const KNOB_GAP: f32 = 6.;
const ROW_GAP: f32 = 4.;

pub fn format_ratio(ratio: f32) -> String {
    if ratio >= 10. {
        format!("{ratio:.0}:1")
    } else {
        format!("{ratio:.1}:1")
    }
}

/// A time in ms: tenths below 10 ms, seconds from one.
pub fn format_time_ms(ms: f32) -> String {
    if ms < 10. {
        format!("{ms:.1} ms")
    } else if ms < 1000. {
        format!("{ms:.0} ms")
    } else {
        format!("{:.1} s", ms / 1000.)
    }
}

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

/// The gain reduction shown: jumps to each new reading's, falls back
/// slowly so it can be read.
#[derive(Default)]
struct Reduction {
    shown: f32,
}

impl Reduction {
    fn update(&mut self, ui: &Ui, tap: &PluginTap, enabled: bool) -> f32 {
        let dt = ui.input(|i| i.stable_dt);
        self.shown = (self.shown - FALL_DB_PER_S * dt).max(0.);
        match tap.reduction.take() {
            Some(db) if enabled => self.shown = self.shown.max(db),
            _ if !enabled => self.shown = 0.,
            _ => {}
        }
        self.shown
    }
}

/// A bar down from the top of `rect`, as deep as `db` is.
fn paint_meter(painter: &Painter, rect: Rect, db: f32, theme: &Theme) {
    painter.rect_filled(rect, 2., theme.bg_deep);
    let y = |db: f32| rect.top() + (db / METER_DB).clamp(0., 1.) * rect.height();
    if db > 0.01 {
        painter.rect_filled(
            Rect::from_min_max(rect.left_top(), pos2(rect.right(), y(db))),
            2.,
            theme.warning,
        );
    }
    for mark in [6., 12., 18.] {
        painter.hline(rect.x_range(), y(mark), Stroke::new(1., theme.grid_beat));
    }
}

/// A knob for a parameter: its name, label, format and whether it's on a
/// log scale.
type KnobSpec<'a> = (&'a str, &'a str, &'a dyn Fn(f32) -> String, bool);

/// Knobs in rows of `columns`, from the top left of `rect`.
fn knob_grid(ui: &mut Ui, rect: Rect, columns: usize, cx: &mut EditorContext, knobs: &[KnobSpec]) {
    for (row, knobs) in knobs.chunks(columns).enumerate() {
        let top = rect.top() + row as f32 * (Knob::HEIGHT + ROW_GAP);
        let mut row_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    pos2(rect.left(), top),
                    vec2(rect.width(), Knob::HEIGHT),
                ))
                .layout(Layout::left_to_right(Align::Min)),
        );
        row_ui.spacing_mut().item_spacing.x = KNOB_GAP;
        for (param, name, format, log) in knobs {
            cx.param_knob(&mut row_ui, param, name, format, *log);
        }
    }
}

fn grid_width(columns: usize) -> f32 {
    columns as f32 * Knob::WIDTH + (columns - 1) as f32 * KNOB_GAP
}

/// Output level against input level, over `range` dB each way.
struct Curve {
    rect: Rect,
    bottom_db: f32,
}

impl Curve {
    fn pos(&self, input: f32, output: f32) -> Pos2 {
        let t = |db: f32| (1. - db / self.bottom_db).clamp(0., 1.);
        pos2(
            self.rect.left() + t(input) * self.rect.width(),
            self.rect.bottom() - t(output) * self.rect.height(),
        )
    }

    /// The grid, the unity line, `out` for each input level, and a dot
    /// where the input's level is now.
    fn paint(
        &self,
        painter: &Painter,
        theme: &Theme,
        color: Color32,
        level: Option<f32>,
        out: impl Fn(f32) -> f32,
    ) {
        painter.rect_filled(self.rect, 2., theme.bg_deep);
        let mut db = -12.;
        while db > self.bottom_db {
            let p = self.pos(db, db);
            painter.vline(p.x, self.rect.y_range(), Stroke::new(1., theme.grid_beat));
            painter.hline(self.rect.x_range(), p.y, Stroke::new(1., theme.grid_beat));
            db -= 12.;
        }
        painter.line_segment(
            [self.pos(self.bottom_db, self.bottom_db), self.pos(0., 0.)],
            Stroke::new(1., theme.grid_bar),
        );
        let steps = self.rect.width().ceil() as usize;
        let points: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let input = self.bottom_db * (1. - i as f32 / steps as f32);
                self.pos(input, out(input))
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.5, color)));
        if let Some(level) = level.filter(|l| *l > self.bottom_db) {
            painter.circle_filled(self.pos(level, out(level)), 3., theme.text);
        }
    }
}

/// What the curve and meter of a gate need.
struct Watch {
    reduction: Reduction,
    input: AudioMetrics,
}

impl Watch {
    fn new() -> Self {
        Self {
            reduction: Reduction::default(),
            input: AudioMetrics::new(),
        }
    }

    /// The input's peak level now, in dBFS, and the reduction to show.
    fn update(&mut self, ui: &Ui, cx: &EditorContext) -> (Option<f32>, f32) {
        let tap = &cx.effect.plugin.tap;
        let reduction = self.reduction.update(ui, tap, cx.enabled());
        if !cx.enabled() {
            return (None, reduction);
        }
        tap.watch_input_for((cx.sample_rate / 2.) as u32);
        self.input.update(&tap.input, false);
        let [l, r] = self.input.peak_db();
        (Some(l.max(r)), reduction)
    }
}

/// A curve at the left, the meter, then knobs in rows of three.
fn curve_layout(ui: &mut Ui) -> (Rect, Rect, Rect) {
    let (body, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let curve = Rect::from_min_size(body.min, vec2(CURVE_WIDTH, body.height()));
    let meter = Rect::from_min_size(
        pos2(curve.right() + KNOB_GAP, body.top()),
        vec2(METER_WIDTH, body.height()),
    );
    let knobs = Rect::from_min_max(pos2(meter.right() + KNOB_GAP, body.top()), body.max);
    (curve, meter, knobs)
}

fn curve_layout_width() -> f32 {
    CURVE_WIDTH + METER_WIDTH + 2. * KNOB_GAP + grid_width(3)
}

fn color(ui: &Ui, cx: &EditorContext) -> Color32 {
    if cx.enabled() {
        ui.app_theme().accent
    } else {
        ui.app_theme().text_disabled
    }
}

/// Seconds of a compressor's history shown.
const HISTORY_S: f32 = 4.;
/// The bottom of a compressor's history, in dB.
const HISTORY_DB: f32 = -60.;
/// Levels marked on a compressor's history.
const HISTORY_MARKS: [f32; 3] = [0., -20., -40.];

/// The levels of one stretch of audio through a compressor, in dB.
struct Reading {
    frames: u32,
    input: f32,
    output: f32,
    reduction: f32,
}

/// The input and output levels over the last few seconds, newest at the
/// right, with the gain taken off down from the top and the threshold,
/// which can be dragged.
pub struct CompressorEditor {
    /// Newest last.
    history: VecDeque<Reading>,
}

impl CompressorEditor {
    pub fn new() -> Self {
        Self {
            history: VecDeque::new(),
        }
    }

    /// Adds what was heard since the last frame, and drops what's out of
    /// sight.
    fn record(&mut self, cx: &EditorContext) {
        let tap = &cx.effect.plugin.tap;
        let (input, output, reduction) = (
            tap.input.take_levels(),
            tap.output.take_levels(),
            tap.reduction.take(),
        );
        if !cx.enabled() {
            return;
        }
        let watch = (cx.sample_rate / 2.) as u32;
        tap.watch_input_for(watch);
        tap.watch_output_for(watch);
        let Some(input) = input else {
            return;
        };
        let peak = |levels: [Level; 2]| gain_to_db(levels[0].peak.max(levels[1].peak));
        let last = self.history.back().map_or(0., |r| r.reduction);
        self.history.push_back(Reading {
            frames: input[0].frames,
            input: peak(input),
            output: output.map_or(HISTORY_DB, peak),
            reduction: reduction.unwrap_or(last),
        });
        let span = (HISTORY_S * cx.sample_rate) as u32;
        let mut frames: u32 = self.history.iter().map(|r| r.frames).sum();
        while frames > span
            && let Some(old) = self.history.pop_front()
        {
            frames -= old.frames;
        }
    }
}

impl EffectEditor for CompressorEditor {
    fn ui(&mut self, ui: &mut Ui, cx: &mut EditorContext) {
        self.record(cx);
        let (rect, response) = graph_rect(ui, Sense::click_and_drag());
        let y = |db: f32| rect.top() + (db / HISTORY_DB).clamp(0., 1.) * rect.height();

        // Drag anywhere: the threshold follows.
        if let Some(threshold) = cx.param("threshold") {
            let before = threshold.get();
            if response.dragged()
                && let Some(pos) = response.interact_pointer_pos()
            {
                let db = (pos.y - rect.top()) / rect.height() * HISTORY_DB;
                threshold.set(db.clamp(threshold.min, threshold.max));
            }
            if response.double_clicked()
                && let Some(value) = cx.effect.kind.initial_value(&cx.effect.plugin, "threshold")
            {
                threshold.set(value);
            }
            if let Some(old) = track_drag(ui, &response, before) {
                cx.edits.push(EffectEdit::Param {
                    id: threshold.id,
                    old,
                    new: threshold.get(),
                });
            }
        }
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
        }

        let theme = ui.app_theme();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2., theme.bg_deep);
        let font = FontId::proportional(font_size::SMALL);
        for mark in HISTORY_MARKS {
            painter.hline(rect.x_range(), y(mark), Stroke::new(1., theme.grid_beat));
            painter.text(
                pos2(rect.right() - 3., y(mark) + 2.),
                Align2::RIGHT_TOP,
                format!("{mark:.0} dB"),
                font.clone(),
                theme.text_muted,
            );
        }

        // Each reading is a column as wide as the audio it's over.
        let (input_color, output_color, reduction_color) = if cx.enabled() {
            (
                theme.bg_deep.lerp_to_gamma(theme.text_muted, 0.6),
                theme.bg_deep.lerp_to_gamma(theme.text_muted, 0.3),
                theme.warning,
            )
        } else {
            (
                theme.bg_deep.lerp_to_gamma(theme.text_disabled, 0.4),
                theme.bg_deep.lerp_to_gamma(theme.text_disabled, 0.2),
                theme.text_disabled,
            )
        };
        let per_frame = rect.width() / (HISTORY_S * cx.sample_rate);
        let mut right = rect.right();
        let mut reduction = Vec::with_capacity(self.history.len() * 2);
        for reading in self.history.iter().rev() {
            let left = right - reading.frames as f32 * per_frame;
            for (db, color) in [(reading.input, input_color), (reading.output, output_color)] {
                painter.rect_filled(
                    Rect::from_min_max(pos2(left, y(db)), pos2(right, rect.bottom())),
                    0.,
                    color,
                );
            }
            let at = y(-reading.reduction);
            reduction.extend([pos2(right, at), pos2(left, at)]);
            right = left;
        }
        painter.add(egui::Shape::line(
            reduction,
            Stroke::new(1.5, reduction_color),
        ));

        if let Some(threshold) = cx.param("threshold") {
            let at = y(threshold.get());
            let color = if cx.enabled() {
                theme.text
            } else {
                theme.text_disabled
            };
            painter.hline(rect.x_range(), at, Stroke::new(1.5, color));
            painter.text(
                pos2(rect.left() + 4., at - 2.),
                Align2::LEFT_BOTTOM,
                format!("Threshold {}", format_db(threshold.get())),
                font.clone(),
                color,
            );
        }
        let shown = self.history.back().map_or(0., |r| r.reduction);
        painter.text(
            rect.left_top() + vec2(4., 3.),
            Align2::LEFT_TOP,
            format!("GR {}", format_reduction(shown)),
            font,
            theme.text,
        );

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = KNOB_GAP;
            cx.param_knob(ui, "attack", "Attack", &format_time_ms, true);
            cx.param_knob(ui, "release", "Release", &format_time_ms, true);
            cx.param_knob(ui, "ratio", "Ratio", &format_ratio, true);
            cx.param_knob(ui, "threshold", "Threshold", &format_db, false);
            cx.param_knob(ui, "knee", "Knee", &format_db, false);
            cx.param_knob(ui, "makeup", "Makeup", &format_db, false);
        });
    }

    fn width(&self) -> f32 {
        grid_width(6) + 12.
    }

    fn expandable(&self) -> bool {
        true
    }
}

pub struct GateEditor {
    watch: Watch,
}

impl GateEditor {
    pub fn new() -> Self {
        Self {
            watch: Watch::new(),
        }
    }
}

impl EffectEditor for GateEditor {
    fn ui(&mut self, ui: &mut Ui, cx: &mut EditorContext) {
        let value = |name| cx.param(name).map_or(0., |p| p.get());
        let (threshold, range) = (value("threshold"), value("range"));
        let (level, reduction) = self.watch.update(ui, cx);
        let (curve, meter, knobs) = curve_layout(ui);
        let theme = ui.app_theme();
        let painter = ui.painter_at(curve.union(meter));
        let bottom_db = GATE_SILENT_DB;
        Curve {
            rect: curve,
            bottom_db,
        }
        .paint(&painter, &theme, color(ui, cx), level, |input| {
            if input >= threshold {
                input
            } else if range <= GATE_SILENT_DB {
                bottom_db
            } else {
                input + range
            }
        });
        paint_meter(&painter, meter, reduction, &theme);

        knob_grid(
            ui,
            knobs,
            3,
            cx,
            &[
                ("threshold", "Threshold", &format_db, false),
                ("range", "Range", &format_range, false),
                ("hold", "Hold", &format_time_ms, false),
                ("attack", "Attack", &format_time_ms, true),
                ("release", "Release", &format_time_ms, true),
            ],
        );
    }

    fn width(&self) -> f32 {
        curve_layout_width() + 12.
    }
}

/// Readings kept for a limiter's history, one a frame.
const HISTORY: usize = 240;

pub struct LimiterEditor {
    reduction: Reduction,
    /// Reductions shown, newest last.
    history: VecDeque<f32>,
}

impl LimiterEditor {
    pub fn new() -> Self {
        Self {
            reduction: Reduction::default(),
            history: VecDeque::with_capacity(HISTORY),
        }
    }
}

impl EffectEditor for LimiterEditor {
    fn ui(&mut self, ui: &mut Ui, cx: &mut EditorContext) {
        let PluginKind::Limiter { lookahead_ms } = cx.effect.plugin.kind else {
            return;
        };
        let reduction = self
            .reduction
            .update(ui, &cx.effect.plugin.tap, cx.enabled());
        if self.history.len() == HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(reduction);

        // The reduction over the last few seconds, down from the top,
        // newest at the right.
        let (rect, _) = graph_rect(ui, Sense::hover());
        let theme = ui.app_theme();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2., theme.bg_deep);
        let y = |db: f32| rect.top() + (db / METER_DB).clamp(0., 1.) * rect.height();
        for mark in [6., 12., 18.] {
            painter.hline(rect.x_range(), y(mark), Stroke::new(1., theme.grid_beat));
            painter.text(
                pos2(rect.left() + 2., y(mark) - 1.),
                Align2::LEFT_BOTTOM,
                format!("-{mark:.0}"),
                FontId::proportional(font_size::SMALL),
                theme.text_muted,
            );
        }
        let step = rect.width() / (HISTORY - 1) as f32;
        let start = rect.right() - (self.history.len() as f32 - 1.) * step;
        let points: Vec<Pos2> = self
            .history
            .iter()
            .enumerate()
            .map(|(i, db)| pos2(start + i as f32 * step, y(*db)))
            .collect();
        let warm = if cx.enabled() {
            theme.warning
        } else {
            theme.text_disabled
        };
        for pair in points.windows(2) {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(pair[0].x, rect.top()),
                    pos2(pair[1].x, rect.top()),
                    pair[1],
                    pair[0],
                ],
                warm.gamma_multiply(0.3),
                Stroke::NONE,
            ));
        }
        painter.add(egui::Shape::line(points, Stroke::new(1.5, warm)));
        painter.text(
            rect.right_top() + vec2(-4., 3.),
            Align2::RIGHT_TOP,
            format_reduction(reduction),
            FontId::proportional(font_size::SMALL),
            theme.text,
        );

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = KNOB_GAP;
            cx.param_knob(ui, "gain", "Gain", &format_db, false);
            cx.param_knob(ui, "ceiling", "Ceiling", &format_db, false);
            cx.param_knob(ui, "release", "Release", &format_time_ms, true);
            if let Some(ms) = cx.setting_knob(
                ui,
                lookahead_ms,
                LIMITER_LOOKAHEADS,
                LIMITER_LOOKAHEAD,
                "Ahead",
                &format_time_ms,
                true,
            ) {
                cx.edits.push(EffectEdit::Setting(Setting::Lookahead(ms)));
            }
        });
    }

    fn width(&self) -> f32 {
        grid_width(4) + 12.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_well() {
        assert_eq!(format_ratio(4.), "4.0:1");
        assert_eq!(format_ratio(20.), "20:1");
        assert_eq!(format_time_ms(0.5), "0.5 ms");
        assert_eq!(format_time_ms(100.), "100 ms");
        assert_eq!(format_time_ms(1500.), "1.5 s");
        assert_eq!(format_range(GATE_SILENT_DB), "-inf dB");
        assert_eq!(format_range(-20.), "-20 dB");
        assert_eq!(format_reduction(0.), "0.0 dB");
        assert_eq!(format_reduction(3.21), "-3.2 dB");
    }
}
