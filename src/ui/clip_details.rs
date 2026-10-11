//! The bottom panel's view of one clip.

use egui::{
    Align, Align2, Color32, CursorIcon, DragValue, FontId, Grid, Label, Layout, Rect, RichText,
    ScrollArea, Sense, Stroke, Ui, UiBuilder, pos2, vec2,
};
use tonique_engine::{edit::ClipId, time::BeatPos};

use crate::{
    core::{
        clip::AudioClip,
        grid::GridService,
        state::{ClipParam, ProjectState},
        track::TrackRow,
    },
    ui::{
        arrangement::grid::PaintGrid,
        theme::{Theme, ThemeExt, font_size, with_alpha},
        waveform::paint_waveform,
        widget::{
            param_slider::{ParamSlider, SliderEdit},
            scroll_bar::{Axis, SCROLLBAR_WIDTH, ScrollBar},
        },
    },
    utils::{format_bars, format_time},
};

/// The clip's properties, left of the editor.
const INSPECTOR_WIDTH: f32 = 210.;
/// The inspector's labels.
const LABEL_WIDTH: f32 = 56.;
const PADDING: f32 = 8.;
const RULER_HEIGHT: f32 = 18.;
/// The inspector's values.
const FIELD_HEIGHT: f32 = 18.;
/// Between the lane and its scrollbar.
const SCROLLBAR_GAP: f32 = 3.;
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

/// Bottom panel view of one clip, as in other DAWs: an inspector of its
/// properties on the left, and on the right its whole source file (the
/// untrimmed clip) on a grid counted from the clip's start (bar 1),
/// zoomable and scrollable, with the part the clip plays between two edges
/// that can be dragged to trim it.
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
        let inspector_width = INSPECTOR_WIDTH.min(rect.width() / 2.);
        let inspector = Rect::from_min_size(rect.min, vec2(inspector_width, rect.height()));
        let view = Rect::from_min_max(pos2(inspector.right() + PADDING, rect.top()), rect.max);
        let ruler = Rect::from_min_max(view.min, pos2(view.right(), view.top() + RULER_HEIGHT));
        let scrollbar =
            Rect::from_min_max(pos2(view.left(), view.bottom() - SCROLLBAR_WIDTH), view.max);
        let lane = Rect::from_min_max(
            pos2(view.left(), ruler.bottom()),
            pos2(view.right(), scrollbar.top() - SCROLLBAR_GAP),
        );
        #[cfg(test)]
        {
            self.lane = lane;
        }

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
        self.scrollbar_ui(ui, scrollbar, lane, clip, bpm);
        self.trim_handles(ui, ruler, lane, clip, state);
        // Show the trim being dragged, here or in the timeline.
        let shown = state.shown_clip(clip);

        // The grid and ruler cover the untrimmed clip: the whole file.
        let theme = ui.app_theme();
        let file = self.file_x(clip, bpm, lane);
        ui.painter_at(ruler).rect_filled(ruler, 0., theme.bg_raised);
        let labels = ruler.intersect(Rect::from_x_y_ranges(file, ruler.y_range()));
        self.grid
            .paint_bar_labels(&ui.painter_at(labels), ruler, &theme);
        self.paint_lane(ui, lane, &shown, track, state, &theme);
        self.paint_edges(ui, ruler, lane, &shown, bpm, &theme);

        ui.scope_builder(UiBuilder::new().max_rect(inspector), |ui| {
            ScrollArea::vertical()
                .id_salt("clip-inspector")
                .auto_shrink(false)
                .show(ui, |ui| inspector_ui(ui, clip, track.color, state));
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

        let (min, max) = self.scroll_range(clip, bpm, lane);
        self.grid.offset.x = self.grid.offset.x.clamp(min, max);
        seek
    }

    /// How far the view scrolls: always keeping part of the file in view.
    fn scroll_range(&self, clip: &AudioClip, bpm: f32, lane: Rect) -> (f32, f32) {
        let file_start = clip.file_start(bpm) - self.origin;
        let half = lane.width() / 2.;
        let min = self.grid.beats_to_width(file_start) - half;
        let max = self
            .grid
            .beats_to_width(file_start + clip.source_beats(bpm))
            - half;
        (min, max.max(min))
    }

    /// Under the lane: the file's extent, and the part of it in view.
    fn scrollbar_ui(&mut self, ui: &mut Ui, track: Rect, lane: Rect, clip: &AudioClip, bpm: f32) {
        let (min, max) = self.scroll_range(clip, bpm, lane);
        let range = max - min;
        if range <= 0. {
            return;
        }
        let mut scroll = self.grid.offset.x - min;
        ui.add(ScrollBar::new(
            "clip-hscroll",
            Axis::Horizontal,
            track,
            &mut scroll,
            range,
            lane.width() / (range + lane.width()),
        ));
        self.grid.offset.x = scroll + min;
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
                clip.gain,
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

/// The clip's properties, in sections as in other DAWs' inspectors: where
/// it is (start, end, length), what it plays (offset in the file, gain),
/// and the file. Values can be dragged, or clicked and typed; edits show
/// while dragged and apply as one undo step once released (or typed). Gain
/// is heard as it's dragged.
fn inspector_ui(ui: &mut Ui, clip: &AudioClip, color: Color32, state: &mut ProjectState) {
    let theme = ui.app_theme();
    let bpm = state.bpm();
    let bpb = state.grid.beats_per_bar();
    let shown = state.shown_clip(clip);
    let start = shown.position.0;
    let end = shown.end(bpm).0;
    let length = end - start;
    let source = shown.source_seconds() as f64;
    let offset = shown.trim_start as f64 * source;
    let audio = &shown.audio;
    let free = ui.input(|i| i.modifiers.alt);
    // Positions and lengths snap to sixteenths, unless Alt is held.
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

    ui.style_mut().override_font_id = Some(FontId::proportional(font_size::LABEL));
    ui.spacing_mut().item_spacing.y = 2.;
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

    section(ui, "Clip");
    rows(ui, "clip", |ui| {
        label(ui, "Start");
        let raw = ui.id().with("start-raw");
        let mut beats = ui.data(|d| d.get_temp(raw)).unwrap_or(start);
        let response = field(
            ui,
            DragValue::new(&mut beats)
                .speed(1. / 16.)
                .range(0.0..=f64::MAX)
                .custom_formatter(|v, _| format_bars(v, bpb))
                .custom_parser(|text| parse_bars(text, bpb)),
        )
        .on_hover_text(format!("{} · bar.beat.sixteenth", seconds(start)));
        keep_raw(ui, raw, &response, beats);
        if let Some(done) = gesture(&response) {
            let mut moved = shown.clone();
            moved.position = BeatPos(snap(beats).max(0.));
            change = Some((moved, done));
        }
        ui.end_row();

        label(ui, "End");
        let raw = ui.id().with("end-raw");
        let mut beats = ui.data(|d| d.get_temp(raw)).unwrap_or(end);
        let response = field(
            ui,
            DragValue::new(&mut beats)
                .speed(1. / 16.)
                .range(start + MIN_LENGTH..=f64::MAX)
                .custom_formatter(|v, _| format_bars(v, bpb))
                .custom_parser(|text| parse_bars(text, bpb)),
        )
        .on_hover_text(format!("{} · bar.beat.sixteenth", seconds(end)));
        keep_raw(ui, raw, &response, beats);
        if let Some(done) = gesture(&response) {
            let at = BeatPos(snap(beats).max(start + MIN_LENGTH));
            change = Some((trimmed(&shown, TrimEdge::End, at, bpm), done));
        }
        ui.end_row();

        label(ui, "Length");
        let raw = ui.id().with("length-raw");
        let mut beats = ui.data(|d| d.get_temp(raw)).unwrap_or(length);
        let response = field(
            ui,
            DragValue::new(&mut beats)
                .speed(1. / 16.)
                .range(MIN_LENGTH..=f64::MAX)
                .custom_formatter(|v, _| format_length(v, bpb))
                .custom_parser(|text| parse_length(text, bpb)),
        )
        .on_hover_text(format!("{} · bars.beats.sixteenths", seconds(length)));
        keep_raw(ui, raw, &response, beats);
        if let Some(done) = gesture(&response) {
            let at = shown.position + snap(beats).max(MIN_LENGTH);
            change = Some((trimmed(&shown, TrimEdge::End, at, bpm), done));
        }
        ui.end_row();
    });

    section(ui, "Audio");
    rows(ui, "audio", |ui| {
        label(ui, "Offset");
        let mut seconds = offset;
        let response = field(
            ui,
            DragValue::new(&mut seconds)
                .speed(0.005)
                .range(0.0..=f64::MAX)
                .custom_formatter(|v, _| format_time(v))
                .custom_parser(parse_time),
        )
        .on_hover_text("Where in the file the clip starts playing");
        if let Some(done) = gesture(&response) {
            change = Some((slipped(&shown, seconds), done));
        }
        ui.end_row();

        // As on tracks: a level filled in the clip's colour, reset by a
        // double-click; heard as it moves, one undo step once released.
        let size = vec2(ui.available_width(), FIELD_HEIGHT);
        label(ui, "Gain");
        let gain = ParamSlider::gain(shown.gain, CLIP_GAIN_RANGE)
            .color(color)
            .size(size)
            .suffix(" dB");
        apply_param(state, clip.id, ClipParam::Gain, gain.show(ui).1);
        ui.end_row();

        label(ui, "Pan");
        let (response, edit) = ParamSlider::pan(shown.pan).color(color).size(size).show(ui);
        response.on_hover_text("Balance: turning to a side lowers the other");
        apply_param(state, clip.id, ClipParam::Pan, edit);
        ui.end_row();
    });

    section(ui, "File");
    rows(ui, "file", |ui| {
        label(ui, "Format");
        value(
            ui,
            format_audio(audio.sample_rate, audio.bit_depth, audio.channels),
        );
        ui.end_row();
        label(ui, "Duration");
        value(ui, format_time(source));
        ui.end_row();
    });

    match change {
        Some((edited, false)) => state.resize_clip(edited),
        Some((edited, true)) => state.commit_resize_clip(edited),
        None => {}
    }
}

/// A section's title, small and muted, with a rule after it.
fn section(ui: &mut Ui, title: &str) {
    ui.add_space(6.);
    let theme = ui.app_theme();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.), Sense::hover());
    let painter = ui.painter();
    let text = painter.text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        title.to_uppercase(),
        FontId::proportional(font_size::SMALL),
        theme.text_muted,
    );
    if text.right() + 6. < rect.right() {
        painter.hline(
            text.right() + 6.0..=rect.right(),
            rect.center().y,
            Stroke::new(1., theme.separator),
        );
    }
}

/// A section's rows: a label, and a value filling the rest.
fn rows(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui)) {
    Grid::new(("clip-inspector", id))
        .num_columns(2)
        .min_col_width(LABEL_WIDTH)
        .spacing([6., 3.])
        .show(ui, add);
}

fn label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(ui.app_theme().text_muted));
}

/// A value field filling the row.
fn field(ui: &mut Ui, value: DragValue) -> egui::Response {
    let width = ui.available_width();
    ui.add_sized(vec2(width, FIELD_HEIGHT), value)
}

/// A value that can't be changed, right of its label.
fn value(ui: &mut Ui, text: String) {
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.add(Label::new(RichText::new(text).color(ui.app_theme().text)).truncate());
    });
}

/// `clip` playing from `offset` seconds into its file, as long as before
/// and where it was: slipped within the file (no further than its ends).
fn slipped(clip: &AudioClip, offset: f64) -> AudioClip {
    let mut clip = clip.clone();
    let source = clip.source_seconds() as f64;
    if source <= 0. {
        return clip;
    }
    let span = (clip.trim_end - clip.trim_start) as f64;
    let start = (offset / source).clamp(0., (1. - span).max(0.));
    clip.trim_start = start as f32;
    clip.trim_end = (start + span) as f32;
    clip
}

/// Apply a gain or pan slider's edit to the clip's parameter.
fn apply_param(state: &mut ProjectState, clip: ClipId, which: ClipParam, edit: SliderEdit) {
    match edit {
        SliderEdit::Moved(value) => state.set_clip_param(clip, which, value),
        SliderEdit::Released { start, value } => state.commit_clip_param(clip, which, start, value),
        SliderEdit::Reset { from, to } => state.commit_clip_param(clip, which, from, to),
        SliderEdit::None => {}
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

/// The gain slider's range, in dB: its bottom is silence, its top the
/// engine's limit (+12 dB).
const CLIP_GAIN_RANGE: std::ops::RangeInclusive<f32> = -40.0..=12.0;

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

/// A length as `bars.beats.sixteenths`, counting from 0: one bar and two
/// beats is `1.2.0`.
fn format_length(beats: f64, beats_per_bar: usize) -> String {
    let sixteenths = (beats * 4.).round().max(0.) as usize;
    let per_bar = beats_per_bar.max(1) * 4;
    format!(
        "{}.{}.{}",
        sixteenths / per_bar,
        sixteenths % per_bar / 4,
        sixteenths % 4
    )
}

/// A time as typed: `1:23.250`, `83.25` (seconds) or `0:05`.
fn parse_time(text: &str) -> Option<f64> {
    let text = text.trim();
    match text.split_once(':') {
        Some((minutes, seconds)) => {
            let minutes: f64 = minutes.trim().parse().ok()?;
            let seconds: f64 = seconds.trim().parse().ok()?;
            (minutes >= 0. && seconds >= 0.).then_some(minutes * 60. + seconds)
        }
        None => text.parse().ok().filter(|s: &f64| *s >= 0.),
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
    fn lengths_count_bars_beats_and_sixteenths_from_zero() {
        assert_eq!(format_length(4., 4), "1.0.0");
        assert_eq!(format_length(6.25, 4), "1.2.1");
        assert_eq!(format_length(3., 4), "0.3.0");
        for beats in [0.25, 6.25, 12.] {
            assert_eq!(parse_length(&format_length(beats, 4), 4), Some(beats));
        }
    }

    #[test]
    fn typed_times_and_slips() {
        assert_eq!(parse_time("1:23.25"), Some(83.25));
        assert_eq!(parse_time("2.5"), Some(2.5));
        assert_eq!(parse_time("-1"), None);
        // The clip plays the file's middle half (1 to 3 s of 4): slip it.
        let c = slipped(&clip(), 0.5);
        assert_eq!((c.trim_start, c.trim_end), (0.125, 0.625));
        assert_eq!(c.position, clip().position, "it stays where it is");
        // No further than the file's ends.
        let c = slipped(&clip(), 10.);
        assert_eq!((c.trim_start, c.trim_end), (0.5, 1.));
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
