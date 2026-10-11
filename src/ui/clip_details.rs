//! The bottom panel's view of one clip.

use egui::{CursorIcon, DragValue, FontId, Grid, Label, Rect, RichText, Sense, Stroke, Ui, pos2};
use tonique_engine::{edit::ClipId, time::BeatPos};

use crate::{
    core::{clip::AudioClip, grid::GridService, state::ProjectState, track::TrackRow},
    ui::{
        arrangement::grid::PaintGrid,
        theme::{Theme, ThemeExt, font_size, with_alpha},
        waveform::paint_waveform,
    },
    utils::{format_bars, format_time},
};

const DETAILS_WIDTH: f32 = 260.;
const LABEL_WIDTH: f32 = 60.;
const PADDING: f32 = 8.;
const RULER_HEIGHT: f32 = 24.;
/// Room left around the clip when fitting it, as a share of its length.
const FIT_MARGIN: f64 = 0.02;
/// Half the width of the area grabbing a trim edge.
const HANDLE_REACH: f32 = 4.;
/// The flags marking the clip's start and end in the ruler: their width
/// (pointing inside the clip) and height.
const FLAG_SIZE: egui::Vec2 = egui::vec2(8., 10.);
/// Shortest a clip can be trimmed to, in beats.
const MIN_LENGTH: f64 = 1. / 16.;

#[derive(Clone, Copy, Debug, PartialEq, Hash)]
enum TrimEdge {
    Start,
    End,
}

/// Bottom panel view of one clip: its whole source file (the untrimmed
/// clip) on a grid counted from the clip's start (bar 1), zoomable and
/// scrollable, with the part the clip plays between two edges that can be
/// dragged to trim it; and the clip's position, length and format.
pub struct ClipDetails {
    /// The view's zoom and scroll, in beats from [`Self::origin`].
    grid: GridService,
    /// The clip the view was fitted to: another one refits it.
    fitted: Option<ClipId>,
    /// Where bar 1 is in the arrangement: the clip's start, as in the edit
    /// (so it holds still while a trim is dragged).
    origin: BeatPos,
    /// The waveform's lane, as last drawn.
    #[cfg(test)]
    lane: Rect,
}

impl ClipDetails {
    pub fn new() -> Self {
        Self {
            // Shows what's before the clip's start, in the file.
            grid: GridService::new().with_negative_offset(),
            fitted: None,
            origin: BeatPos::ZERO,
            #[cfg(test)]
            lane: Rect::NOTHING,
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        clip: &AudioClip,
        track: &TrackRow,
        state: &mut ProjectState,
    ) {
        let rect = ui.available_rect_before_wrap().shrink(PADDING);
        let details_width = DETAILS_WIDTH.min(rect.width() / 2.);
        let view = Rect::from_min_max(
            rect.min,
            pos2(rect.right() - details_width - PADDING, rect.bottom()),
        );
        let ruler = Rect::from_min_max(view.min, pos2(view.right(), view.top() + RULER_HEIGHT));
        let lane = Rect::from_min_max(pos2(view.left(), ruler.bottom()), view.max);
        #[cfg(test)]
        {
            self.lane = lane;
        }
        let details = Rect::from_min_max(pos2(view.right() + PADDING, rect.top()), rect.max);

        let bpm = state.bpm();
        if self.fitted == Some(clip.id) && clip.position != self.origin {
            // Its start moved (a trim or a move was applied): bar 1 follows,
            // and the audio stays where it was on screen.
            self.grid.offset.x -= self.grid.beats_to_width(clip.position - self.origin);
        }
        self.origin = clip.position;
        if self.fitted != Some(clip.id) {
            self.fit(clip, bpm, lane.width());
            self.fitted = Some(clip.id);
        }
        if let Some(at) = self.navigate(ui, ruler, lane, clip, bpm) {
            state.seek(at);
        }
        self.trim_handles(ui, ruler, lane, clip, state);
        // Show the trim being dragged, here or in the timeline.
        let shown = state.shown_clip(clip);

        // The grid and ruler cover the untrimmed clip: the whole file.
        let theme = ui.app_theme();
        let file = self.file_x(clip, bpm, lane);
        ui.painter_at(ruler).rect_filled(ruler, 0., theme.bg_raised);
        let labels = ruler.intersect(Rect::from_x_y_ranges(file, ruler.y_range()));
        self.grid
            .paint_labels(&ui.painter_at(labels), ruler, bpm, &theme);
        self.paint_lane(ui, lane, &shown, track, state, &theme);
        self.paint_edges(ui, ruler, lane, &shown, bpm, &theme);

        ui.scope_builder(egui::UiBuilder::new().max_rect(details), |ui| {
            details_grid(ui, clip, state);
        });
        ui.allocate_rect(rect, Sense::hover());
    }

    /// Where `at` (in the arrangement) is drawn.
    fn x(&self, at: BeatPos, lane: Rect) -> f32 {
        self.grid.beats_to_x(BeatPos(at - self.origin), lane)
    }

    /// The arrangement position drawn at `x`.
    fn at(&self, x: f32, lane: Rect) -> BeatPos {
        self.origin + self.grid.x_to_beats(x, lane).0
    }

    /// Where the whole file is drawn, left to right.
    fn file_x(&self, clip: &AudioClip, bpm: f32, lane: Rect) -> std::ops::RangeInclusive<f32> {
        let start = clip.file_start(bpm);
        self.x(start, lane)..=self.x(start + clip.source_beats(bpm), lane)
    }

    /// Zoom and scroll to the part of the file the clip plays.
    fn fit(&mut self, clip: &AudioClip, bpm: f32, width: f32) {
        let length = clip.end(bpm) - clip.position;
        let margin = length * FIT_MARGIN;
        self.grid.fit(
            BeatPos(clip.position - self.origin - margin),
            BeatPos(clip.end(bpm) - self.origin + margin),
            width,
        );
    }

    /// Ctrl+wheel or pinch zooms, like the timeline, and so does the wheel
    /// over the ruler. The wheel or a drag over the lane scrolls.
    /// Double-clicking the lane returns where to seek to; double-clicking
    /// the ruler fits the clip again.
    fn navigate(
        &mut self,
        ui: &Ui,
        ruler: Rect,
        lane: Rect,
        clip: &AudioClip,
        bpm: f32,
    ) -> Option<BeatPos> {
        let response = ui
            .interact(lane, ui.id().with("clip-lane"), Sense::click_and_drag())
            .on_hover_cursor(CursorIcon::Grab);
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let over_ruler = pointer.is_some_and(|p| ruler.contains(p));
        let over_lane = pointer.is_some_and(|p| lane.contains(p));

        let zoom = ui.input(|i| i.zoom_delta());
        let scroll = ui.input(|i| i.smooth_scroll_delta);
        if let Some(pointer) = pointer.filter(|_| over_ruler || over_lane) {
            if zoom != 1. {
                self.grid.zoom_by(zoom, pointer.x, lane);
            } else if over_ruler && scroll.y != 0. {
                self.grid.zoom_around(scroll.y, pointer.x, lane);
            } else if over_lane {
                // Only horizontal scrolling here: use the vertical wheel too.
                self.grid.offset.x -= if scroll.x != 0. { scroll.x } else { scroll.y };
            }
        }
        if response.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            self.grid.offset.x -= response.drag_delta().x;
        }
        let ruler_response = ui
            .interact(ruler, ui.id().with("clip-ruler"), Sense::click())
            .on_hover_text("Bars from the clip's start. Double-click to fit the clip.");
        if ruler_response.double_clicked() {
            self.fit(clip, bpm, lane.width());
        }
        let seek = response
            .double_clicked()
            .then(|| response.interact_pointer_pos())
            .flatten()
            .map(|p| self.at(p.x, lane));

        // Keep part of the file in view.
        let file_start = clip.file_start(bpm) - self.origin;
        let half = lane.width() / 2.;
        let min = self.grid.beats_to_width(file_start) - half;
        let max = self
            .grid
            .beats_to_width(file_start + clip.source_beats(bpm))
            - half;
        self.grid.offset.x = self.grid.offset.x.clamp(min, max.max(min));
        seek
    }

    /// Drag the edges of the played part, in the lane or by their flags in
    /// the ruler, to trim the clip. Snaps to the grid (counted from the
    /// clip's start) unless Alt is held, like the timeline; commits on
    /// release.
    fn trim_handles(
        &self,
        ui: &Ui,
        ruler: Rect,
        lane: Rect,
        clip: &AudioClip,
        state: &mut ProjectState,
    ) {
        let bpm = state.bpm();
        let shown = state.shown_clip(clip);
        let edges = [
            (TrimEdge::Start, shown.position),
            (TrimEdge::End, shown.end(bpm)),
        ];
        let view = ruler.union(lane);
        for (edge, at) in edges {
            let x = self.x(at, lane);
            let line = Rect::from_x_y_ranges(x - HANDLE_REACH..=x + HANDLE_REACH, view.y_range());
            let flag = flag_rect(edge, x, ruler);
            let id = ui.id().with(("trim", edge));
            let response = ui
                .interact(line.intersect(view), id, Sense::drag())
                .union(ui.interact(flag.intersect(view), id.with("flag"), Sense::drag()))
                .on_hover_and_drag_cursor(CursorIcon::ResizeHorizontal);
            if response.hovered() || response.dragged() {
                let theme = ui.app_theme();
                ui.painter_at(lane)
                    .rect_filled(line, 1., theme.hover_overlay);
            }
            if response.dragged()
                && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
            {
                let beats = self.grid.x_to_beats(pointer.x, lane);
                let beats = if ui.input(|i| i.modifiers.alt) {
                    beats
                } else {
                    self.grid.snap_at_grid(beats)
                };
                state.resize_clip(trimmed(clip, edge, self.origin + beats.0, bpm));
            }
            if response.drag_stopped() {
                state.commit_resize_clip(state.shown_clip(clip));
            }
        }
    }

    /// The file's waveform on the grid, dimmed outside the part the clip
    /// plays, and the playhead.
    fn paint_lane(
        &self,
        ui: &Ui,
        lane: Rect,
        clip: &AudioClip,
        track: &TrackRow,
        state: &ProjectState,
        theme: &Theme,
    ) {
        let bpm = state.bpm();
        let painter = ui.painter_at(lane);
        painter.rect_filled(lane, 0., theme.bg_deep);
        let file = Rect::from_x_y_ranges(self.file_x(clip, bpm, lane), lane.y_range());
        self.grid
            .paint_grid(&ui.painter_at(lane.intersect(file)), lane, theme);

        if let Some(frames) = clip.audio.total_frames() {
            paint_waveform(
                &painter,
                file,
                file.intersect(lane),
                &clip.audio.data,
                0.0..frames,
                clip.audio.channels >= 2,
                track.color,
            );
        }

        let (start, end) = (self.x(clip.position, lane), self.x(clip.end(bpm), lane));
        let dim = with_alpha(theme.bg_deep, 190);
        for part in [file.left()..=start, end..=file.right()] {
            painter.rect_filled(Rect::from_x_y_ranges(part, lane.y_range()), 0., dim);
        }
        painter.vline(
            self.x(state.playhead(), lane),
            lane.y_range(),
            Stroke::new(1., theme.playhead),
        );
    }

    /// The clip's edges across the lane, with a flag in the ruler pointing
    /// inside the clip: in the text colour, to stand out on the waveform.
    fn paint_edges(
        &self,
        ui: &Ui,
        ruler: Rect,
        lane: Rect,
        clip: &AudioClip,
        bpm: f32,
        theme: &Theme,
    ) {
        let view = ruler.union(lane);
        let painter = ui.painter_at(view);
        let color = theme.text;
        for (edge, at) in [
            (TrimEdge::Start, clip.position),
            (TrimEdge::End, clip.end(bpm)),
        ] {
            let x = self.x(at, lane);
            painter.vline(x, ruler.bottom()..=lane.bottom(), Stroke::new(1.5, color));
            let flag = flag_rect(edge, x, ruler);
            let tip = match edge {
                TrimEdge::Start => flag.right_top() + egui::vec2(0., FLAG_SIZE.y / 2.),
                TrimEdge::End => flag.left_top() + egui::vec2(0., FLAG_SIZE.y / 2.),
            };
            let (outer_top, outer_bottom) = match edge {
                TrimEdge::Start => (flag.left_top(), flag.left_bottom()),
                TrimEdge::End => (flag.right_top(), flag.right_bottom()),
            };
            painter.add(egui::Shape::convex_polygon(
                vec![outer_top, tip, outer_bottom],
                color,
                Stroke::NONE,
            ));
            painter.vline(
                x,
                flag.y_range().min..=ruler.bottom(),
                Stroke::new(1.5, color),
            );
        }
    }
}

/// Where the flag of `edge`, drawn at `x`, sits in the ruler: at its bottom,
/// on the clip's side of the edge.
fn flag_rect(edge: TrimEdge, x: f32, ruler: Rect) -> Rect {
    let top = ruler.bottom() - FLAG_SIZE.y;
    let left = match edge {
        TrimEdge::Start => x,
        TrimEdge::End => x - FLAG_SIZE.x,
    };
    Rect::from_min_size(pos2(left, top), FLAG_SIZE)
}

/// `clip` with `edge` moved to `at`. The audio stays where it is in the
/// arrangement, so trimming the start moves the clip's position; the clip
/// keeps at least [`MIN_LENGTH`], and its start stays within the file and
/// after the first beat.
fn trimmed(clip: &AudioClip, edge: TrimEdge, at: BeatPos, bpm: f32) -> AudioClip {
    let mut clip = clip.clone();
    match edge {
        TrimEdge::Start => clip.trim_start_at(at.min(clip.end(bpm) - MIN_LENGTH), bpm),
        TrimEdge::End => clip.trim_end_at(at.max(clip.position + MIN_LENGTH), bpm),
    }
    clip
}

/// The clip's name; its position, length and gain, to drag or click and
/// type; what of the file it plays, and the file's format. Edits show while
/// dragged and apply as one undo step once released (or typed).
fn details_grid(ui: &mut Ui, clip: &AudioClip, state: &mut ProjectState) {
    let theme = ui.app_theme();
    let bpm = state.bpm();
    let bpb = state.grid.beats_per_bar();
    let shown = state.shown_clip(clip);
    let source = shown.source_seconds();
    let position = shown.position.0;
    let length = shown.end(bpm) - shown.position;
    let audio = &shown.audio;
    // Positions and lengths snap to sixteenths, unless Alt is held.
    let free = ui.input(|i| i.modifiers.alt);
    let snap = |beats: f64| {
        if free {
            beats
        } else {
            (beats * 4.).round() / 4.
        }
    };
    let seconds = |beats: f64| format_time(beats * 60. / bpm as f64);
    // The clip as edited, and whether the edit is done.
    let mut change: Option<(AudioClip, bool)> = None;

    ui.add(
        Label::new(
            RichText::new(&audio.name)
                .size(font_size::BODY)
                .strong()
                .color(theme.text),
        )
        .truncate(),
    )
    .on_hover_text(audio.path.display().to_string());
    ui.add_space(4.);
    Grid::new(("clip-details", clip.id))
        .num_columns(2)
        .min_col_width(LABEL_WIDTH)
        .spacing([8., 3.])
        .show(ui, |ui| {
            ui.style_mut().override_font_id = Some(FontId::proportional(font_size::LABEL));
            let label = |ui: &mut Ui, text: &str| {
                ui.label(RichText::new(text).color(theme.text_muted));
            };
            let muted = |ui: &mut Ui, text: String| {
                ui.add(Label::new(RichText::new(text).color(theme.text_muted)).truncate());
            };

            label(ui, "Position");
            ui.horizontal(|ui| {
                let raw = ui.id().with("position-raw");
                let mut beats = ui.data(|d| d.get_temp(raw)).unwrap_or(position);
                let response = ui
                    .add(
                        DragValue::new(&mut beats)
                            .speed(1. / 16.)
                            .range(0.0..=f64::MAX)
                            .custom_formatter(|v, _| format_bars(v, bpb))
                            .custom_parser(|text| parse_bars(text, bpb)),
                    )
                    .on_hover_text("Drag, or click and type bar.beat.sixteenth");
                keep_raw(ui, raw, &response, beats);
                if let Some(done) = gesture(&response) {
                    let mut moved = shown.clone();
                    moved.position = BeatPos(snap(beats).max(0.));
                    change = Some((moved, done));
                }
                muted(ui, seconds(position));
            });
            ui.end_row();

            label(ui, "Length");
            ui.horizontal(|ui| {
                let raw = ui.id().with("length-raw");
                let mut beats = ui.data(|d| d.get_temp(raw)).unwrap_or(length);
                let response = ui
                    .add(
                        DragValue::new(&mut beats)
                            .speed(1. / 16.)
                            .range(MIN_LENGTH..=f64::MAX)
                            .custom_formatter(|v, _| format_length(v, bpb))
                            .custom_parser(|text| parse_length(text, bpb)),
                    )
                    .on_hover_text("Drag, or click and type bars.beats.sixteenths");
                keep_raw(ui, raw, &response, beats);
                if let Some(done) = gesture(&response) {
                    let mut trimmed = shown.clone();
                    let beats = snap(beats).max(MIN_LENGTH);
                    trimmed.trim_end_at(trimmed.position + beats, bpm);
                    change = Some((trimmed, done));
                }
                muted(ui, seconds(length));
            });
            ui.end_row();

            label(ui, "Gain");
            let mut db = gain_to_db(shown.gain);
            let response = ui.add(
                DragValue::new(&mut db)
                    .speed(0.1)
                    .range(SILENT_DB..=MAX_GAIN_DB)
                    .custom_formatter(|db, _| format_gain(db as f32))
                    .custom_parser(parse_gain),
            );
            if let Some(done) = gesture(&response) {
                let mut louder = shown.clone();
                louder.gain = db_to_gain(db);
                change = Some((louder, done));
            }
            ui.end_row();

            label(ui, "Plays");
            muted(
                ui,
                format!(
                    "{} – {} of {}",
                    format_time((shown.trim_start * source) as f64),
                    format_time((shown.trim_end * source) as f64),
                    format_time(source as f64)
                ),
            );
            ui.end_row();

            label(ui, "Format");
            muted(
                ui,
                format_audio(audio.sample_rate, audio.bit_depth, audio.channels),
            );
            ui.end_row();
        });

    match change {
        Some((edited, false)) => state.resize_clip(edited),
        Some((edited, true)) => {
            let gain = edited.gain;
            state.commit_resize_clip(edited);
            state.commit_clip_gain(clip.id, gain);
        }
        None => {}
    }
}

/// Keep a dragged value as it is, before snapping, so that snapping what's
/// shown doesn't hold back small drags.
fn keep_raw(ui: &Ui, id: egui::Id, response: &egui::Response, value: f64) {
    if response.dragged() {
        ui.data_mut(|d| d.insert_temp(id, value));
    } else {
        ui.data_mut(|d| d.remove::<f64>(id));
    }
}

/// Where a drag value's edit is at: `Some(false)` while dragged,
/// `Some(true)` once released or typed, `None` when untouched.
fn gesture(response: &egui::Response) -> Option<bool> {
    if response.drag_stopped() {
        Some(true)
    } else if response.changed() {
        Some(!response.dragged())
    } else {
        None
    }
}

/// Gain at the bottom of the range: silence.
const SILENT_DB: f32 = -60.;
const MAX_GAIN_DB: f32 = 12.;

fn gain_to_db(gain: f32) -> f32 {
    if gain <= 0. {
        SILENT_DB
    } else {
        (20. * gain.log10()).max(SILENT_DB)
    }
}

fn db_to_gain(db: f32) -> f32 {
    if db <= SILENT_DB {
        0.
    } else {
        10f32.powf(db / 20.)
    }
}

/// `+3.0 dB`, or `-inf dB` for silence.
fn format_gain(db: f32) -> String {
    if db <= SILENT_DB {
        "-inf dB".into()
    } else {
        format!("{db:+.1} dB")
    }
}

/// A gain as typed: `-6`, `+3 dB`, `-inf`.
fn parse_gain(text: &str) -> Option<f64> {
    let text = text
        .trim()
        .trim_end_matches("dB")
        .trim_end_matches("db")
        .trim();
    if text.eq_ignore_ascii_case("-inf") {
        return Some(SILENT_DB as f64);
    }
    text.parse().ok()
}

/// A position as typed, `bar.beat.sixteenth` from 1 (`3.2.1`; missing
/// parts are the first), in beats.
fn parse_bars(text: &str, beats_per_bar: usize) -> Option<f64> {
    let parts = parse_parts(text)?;
    let part = |i: usize| parts.get(i).copied().unwrap_or(1.);
    if parts.is_empty() || parts.iter().any(|p| *p < 1.) {
        return None;
    }
    Some((part(0) - 1.) * beats_per_bar as f64 + (part(1) - 1.) + (part(2) - 1.) / 4.)
}

/// A length as typed, `bars.beats.sixteenths` (`1.2.0`; missing parts are
/// none), in beats.
fn parse_length(text: &str, beats_per_bar: usize) -> Option<f64> {
    let parts = parse_parts(text)?;
    let part = |i: usize| parts.get(i).copied().unwrap_or(0.);
    if parts.is_empty() || parts.iter().any(|p| *p < 0.) {
        return None;
    }
    Some(part(0) * beats_per_bar as f64 + part(1) + part(2) / 4.)
}

/// Up to three numbers separated by dots.
fn parse_parts(text: &str) -> Option<Vec<f64>> {
    let parts: Vec<f64> = text
        .trim()
        .split('.')
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;
    (parts.len() <= 3).then_some(parts)
}

/// Length as bars, beats and sixteenths: `1 bar 2 beats`, `3 beats`.
fn format_length(beats: f64, beats_per_bar: usize) -> String {
    let sixteenths = (beats * 4.).round().max(0.) as usize;
    let per_bar = beats_per_bar.max(1) * 4;
    let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    let parts: Vec<String> = [
        (sixteenths / per_bar, "bar"),
        (sixteenths % per_bar / 4, "beat"),
        (sixteenths % 4, "sixteenth"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, word)| plural(n, word))
    .collect();
    if parts.is_empty() {
        "0 beats".into()
    } else {
        parts.join(" ")
    }
}

/// `48 kHz · 24-bit · stereo`.
fn format_audio(sample_rate: u32, bit_depth: Option<u32>, channels: u16) -> String {
    let rate = sample_rate as f32 / 1000.;
    let rate = if rate.fract() == 0. {
        format!("{rate:.0} kHz")
    } else {
        format!("{rate:.1} kHz")
    };
    let channels = match channels {
        1 => "mono".to_string(),
        2 => "stereo".to_string(),
        n => format!("{n} channels"),
    };
    match bit_depth {
        Some(bits) => format!("{rate} · {bits}-bit · {channels}"),
        None => format!("{rate} · {channels}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clip at beat 10 playing the middle half of an 8-beat file (at 120
    /// bpm): beats 2 to 6 of the file.
    fn clip() -> AudioClip {
        use crate::analysis::AudioInfo;
        let audio = AudioInfo {
            name: "a.wav".into(),
            duration: Some(std::time::Duration::from_secs(4)),
            data: Default::default(),
            sample_rate: 48000,
            channels: 2,
            bit_depth: None,
            num_samples: None,
            path: Default::default(),
        };
        let mut clip = AudioClip::new(ClipId(1), audio, BeatPos(10.));
        clip.trim_start = 0.25;
        clip.trim_end = 0.75;
        clip
    }

    #[test]
    fn the_file_starts_before_the_clip_by_what_is_trimmed() {
        assert_eq!(clip().source_beats(120.), 8.);
        assert_eq!(clip().file_start(120.), BeatPos(8.));
    }

    #[test]
    fn trimming_the_start_keeps_the_audio_in_place() {
        let t = trimmed(&clip(), TrimEdge::Start, BeatPos(9.), 120.);
        assert_eq!((t.trim_start, t.position), (0.125, BeatPos(9.)));
        assert_eq!(t.trim_end, 0.75);
        // No further than the file's start...
        let t = trimmed(&clip(), TrimEdge::Start, BeatPos(5.), 120.);
        assert_eq!((t.trim_start, t.position), (0., BeatPos(8.)));
        // ...nor past the end, minus the minimum length.
        let t = trimmed(&clip(), TrimEdge::Start, BeatPos(15.), 120.);
        assert_eq!(t.trim_start, (0.75 - MIN_LENGTH / 8.) as f32);
    }

    #[test]
    fn trimming_the_start_stops_at_the_arrangement_start() {
        let mut near_zero = clip();
        near_zero.position = BeatPos(1.); // the file would start at beat -1
        let t = trimmed(&near_zero, TrimEdge::Start, BeatPos(-1.), 120.);
        assert_eq!((t.trim_start, t.position), (0.125, BeatPos::ZERO));
    }

    #[test]
    fn trimming_the_end() {
        let t = trimmed(&clip(), TrimEdge::End, BeatPos(15.), 120.);
        assert_eq!(
            (t.trim_end, t.position, t.trim_start),
            (0.875, BeatPos(10.), 0.25)
        );
        assert_eq!(
            trimmed(&clip(), TrimEdge::End, BeatPos(28.), 120.).trim_end,
            1.
        );
        assert_eq!(
            trimmed(&clip(), TrimEdge::End, BeatPos(8.), 120.).trim_end,
            (0.25 + MIN_LENGTH / 8.) as f32
        );
    }

    #[test]
    fn typed_positions_and_lengths_read_as_shown() {
        assert_eq!(parse_bars("3.2.3", 4), Some(9.5));
        assert_eq!(parse_bars("1.1.1", 4), Some(0.));
        assert_eq!(parse_bars("2", 4), Some(4.), "missing parts are the first");
        assert_eq!(parse_bars("0.1.1", 4), None, "bars count from 1");
        assert_eq!(parse_bars("1.2.3.4", 4), None);
        assert_eq!(parse_bars("x", 4), None);
        assert_eq!(parse_length("1.2.1", 4), Some(6.25));
        assert_eq!(parse_length("3", 4), Some(12.), "bars first");
        assert_eq!(parse_length("0.0.2", 4), Some(0.5));
        for beats in [0., 6.25, 9.5] {
            assert_eq!(parse_bars(&format_bars(beats, 4), 4), Some(beats));
        }
    }

    #[test]
    fn gains_read_in_decibels() {
        assert_eq!(format_gain(gain_to_db(1.)), "+0.0 dB");
        assert_eq!(format_gain(gain_to_db(0.)), "-inf dB");
        assert_eq!(db_to_gain(SILENT_DB), 0.);
        assert!((db_to_gain(-6.0206) - 0.5).abs() < 1e-4);
        assert_eq!(parse_gain("-6 dB"), Some(-6.));
        assert_eq!(parse_gain("+3"), Some(3.));
        assert_eq!(parse_gain("-inf"), Some(SILENT_DB as f64));
    }

    #[test]
    fn lengths_read_naturally() {
        assert_eq!(format_length(4., 4), "1 bar");
        assert_eq!(format_length(6.25, 4), "1 bar 2 beats 1 sixteenth");
        assert_eq!(format_length(3., 4), "3 beats");
        assert_eq!(format_length(0., 4), "0 beats");
    }

    #[test]
    fn formats_read_naturally() {
        assert_eq!(
            format_audio(44100, Some(16), 2),
            "44.1 kHz · 16-bit · stereo"
        );
        assert_eq!(format_audio(48000, None, 1), "48 kHz · mono");
    }

    /// Drives the view of one clip headless, with real pointer events.
    struct Harness {
        ctx: egui::Context,
        details: ClipDetails,
        state: ProjectState,
        clip: ClipId,
    }

    impl Harness {
        /// A 4-beat file at beat 4, its first and last beat trimmed away.
        fn new() -> Self {
            use crate::analysis::AudioInfo;
            use tonique_engine::engine::{Engine, EngineConfig};
            let (engine, _processor) = Engine::new(EngineConfig::default());
            let mut state = ProjectState::new(engine);
            let track = state.add_track();
            let clip = state.new_clip_id();
            let mut audio = AudioClip::new(clip, AudioInfo::silent(2.), BeatPos(4.));
            audio.trim_start = 0.25;
            audio.trim_end = 0.75;
            audio.position = BeatPos(5.);
            state.add_clips(&track, vec![audio]);
            let ctx = egui::Context::default();
            ctx.set_fonts(crate::ui::font::fonts());
            let mut harness = Self {
                ctx,
                details: ClipDetails::new(),
                state,
                clip,
            };
            harness.frame(Vec::new());
            harness
        }

        fn clip(&self) -> AudioClip {
            self.state.find_clip(self.clip).unwrap().1
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 200.),
                )),
                time: Some(self.ctx.input(|i| i.time) + 1.),
                events,
                ..Default::default()
            };
            let clip = self.clip();
            let (details, state) = (&mut self.details, &mut self.state);
            let mut output = self.ctx.run_ui(input, |ui| {
                let track = state.tracks().next().unwrap();
                details.ui(ui, &clip, &track, state);
            });
            output.textures_delta.clear();
        }

        fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            self.frame(vec![egui::Event::PointerMoved(from), button(from, true)]);
            for step in 1..=4 {
                let at = from.lerp(to, step as f32 / 4.);
                self.frame(vec![egui::Event::PointerMoved(at)]);
            }
            self.frame(vec![button(to, false)]);
            self.frame(Vec::new());
        }

        /// The middle of the lane at `at` in the arrangement.
        fn at(&self, at: BeatPos) -> egui::Pos2 {
            let lane = self.details.lane;
            egui::pos2(self.details.x(at, lane), lane.center().y)
        }
    }

    #[test]
    fn dragging_an_edge_trims_the_clip_in_one_step() {
        let mut h = Harness::new();
        let end = h.clip().end(120.);
        assert_eq!(end, BeatPos(7.));
        // Out past the file's end: it stops there, at beat 8.
        let (from, to) = (h.at(end), h.at(BeatPos(9.)));
        h.drag(from, to);
        assert_eq!(h.clip().end(120.), BeatPos(8.));
        h.state.undo();
        assert_eq!(h.clip().end(120.), end, "one step");
    }

    #[test]
    fn trimming_the_start_moves_bar_one_but_not_the_audio() {
        let mut h = Harness::new();
        let file_start = h.clip().file_start(120.);
        let before = h.at(file_start).x;
        assert_eq!(h.details.origin, BeatPos(5.));
        let (from, to) = (h.at(BeatPos(5.)), h.at(BeatPos(6.)));
        h.drag(from, to);
        assert_eq!(h.clip().position, BeatPos(6.));
        assert_eq!(h.details.origin, BeatPos(6.), "bar 1 is the new start");
        assert!(
            (h.at(file_start).x - before).abs() < 0.01,
            "the audio stayed"
        );
    }
}
