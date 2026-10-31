//! The bottom panel's view of one clip.

use crate::ui::font::semibold;
use egui::{
    Align2, Color32, CursorIcon, DragValue, FontId, Label, Rect, Response, RichText, ScrollArea,
    Sense, Shape, Stroke, Ui, UiBuilder, pos2, vec2,
};
use tonique_engine::{
    edit::{ClipId, ClipLoop},
    time::{BeatPos, TimeSignature},
};

use crate::{
    core::{
        clip::{AudioClip, MIN_LENGTH},
        grid::GridService,
        state::{ClipParam, ProjectState},
        track::TrackRow,
    },
    ui::{
        arrangement::grid::PaintGrid,
        theme::{Theme, ThemeExt, font_size, with_alpha},
        waveform::paint_waveform,
        widget::{
            flat_button::FlatButton,
            param_slider::{ParamSlider, SliderEdit},
            scroll_bar::{Axis, SCROLLBAR_WIDTH, ScrollBar},
        },
    },
    utils::{format_bars, format_time, parse_bars, parse_parts},
};

/// The clip's properties, left of the editor.
const INSPECTOR_WIDTH: f32 = 210.;
/// Between the inspector's cells, side by side.
const CELL_GAP: f32 = 4.;
/// Above each section of the inspector.
const SECTION_GAP: f32 = 4.;
/// The loop's on/off switch, on its section's title row.
const LOOP_SWITCH_WIDTH: f32 = 28.;
const PADDING: f32 = 8.;
/// The ruler: the loop's bracket along its top, the bars, and the markers'
/// flags along its bottom.
const RULER_HEIGHT: f32 = 24.;
const LOOP_BAR_HEIGHT: f32 = 6.;
/// The inspector's values.
const FIELD_HEIGHT: f32 = 18.;
/// Between the lane and its scrollbar.
const SCROLLBAR_GAP: f32 = 3.;
/// Room left around the clip when fitting it, as a share of its length.
const FIT_MARGIN: f64 = 0.02;
/// Half the width of the area grabbing a marker or a loop edge.
const HANDLE_REACH: f32 = 4.;
/// The flags marking the clip's start and end in the ruler: their width
/// (pointing inside the clip) and height.
const FLAG_SIZE: egui::Vec2 = egui::vec2(8., 10.);

#[derive(Clone, Copy, Debug, PartialEq, Hash)]
enum TrimEdge {
    Start,
    End,
}

/// What a drag in the ruler or the lane moves.
#[derive(Clone, Copy, Debug, PartialEq, Hash)]
enum Handle {
    /// Where playback starts in the file: slips the audio.
    StartMarker,
    /// Where the clip ends, without a loop.
    EndMarker,
    LoopStart,
    LoopEnd,
    /// The whole loop.
    Loop,
}

/// Bottom panel view of one clip: an inspector of its properties on the
/// left, and on the right its whole source file on a grid counted from the
/// clip's bar 1 (the file's start unless moved), zoomable and scrollable. Markers
/// in the ruler show where playback starts and (without a loop) ends, and
/// a bracket the loop; all can be dragged.
pub struct ClipDetails {
    /// The view's zoom and scroll, in beats from [`Self::origin`].
    grid: GridService,
    /// The clip the view was fitted to: another one refits it.
    fitted: Option<ClipId>,
    /// Where bar 1 is in the file, in seconds: the clip's, as last drawn.
    origin: f64,
    /// The waveform's lane, as last drawn.
    #[cfg(test)]
    lane: Rect,
}

impl ClipDetails {
    pub fn new() -> Self {
        Self {
            // Shows what's before the start marker, in the file.
            grid: GridService::new().with_negative_offset(),
            fitted: None,
            origin: 0.,
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
        // Bars as the project counts them.
        self.grid.set_time_signature(state.time_signature());
        if self.fitted == Some(clip.id) && clip.origin != self.origin {
            // Bar 1 moved: the grid follows, and the audio stays where it
            // was on screen.
            self.grid.offset.x -= self
                .grid
                .beats_to_width(to_beats(clip.origin - self.origin, bpm));
        }
        self.origin = clip.origin;
        if self.fitted != Some(clip.id) {
            self.fit(clip, bpm, lane.width());
            self.fitted = Some(clip.id);
        }
        let (seek, ruler_response) = self.navigate(ui, ruler, lane, clip, bpm);
        if let Some(at) = seek {
            state.seek(at);
        }
        self.bar_one_menu(ui, &ruler_response, lane, clip, state);
        self.scrollbar_ui(ui, scrollbar, lane, clip, bpm);
        self.handles(ui, ruler, lane, clip, state);
        // As being dragged, here or in the timeline.
        let shown = state.shown_clip(clip);

        let theme = ui.app_theme();
        let file = self.file_x(&shown, bpm, lane);
        ui.painter_at(ruler).rect_filled(ruler, 0., theme.bg_raised);
        let bars = Rect::from_min_max(
            pos2(ruler.left(), ruler.top() + LOOP_BAR_HEIGHT),
            pos2(ruler.right(), ruler.bottom() - FLAG_SIZE.y),
        );
        let labels = ruler.intersect(Rect::from_x_y_ranges(file, ruler.y_range()));
        self.grid
            .paint_bar_labels(&ui.painter_at(labels), bars, &theme);
        self.paint_lane(ui, lane, &shown, track, state, &theme);
        self.paint_markers(ui, ruler, lane, &shown, track.color, bpm, &theme);

        ui.scope_builder(UiBuilder::new().max_rect(inspector), |ui| {
            ScrollArea::vertical()
                .id_salt("clip-inspector")
                .auto_shrink(false)
                .show(ui, |ui| inspector_ui(ui, clip, track.color, state));
        });
        ui.allocate_rect(rect, Sense::hover());
    }

    /// Where `file` seconds into the file are drawn.
    fn x(&self, file: f64, bpm: f32, lane: Rect) -> f32 {
        self.grid
            .beats_to_x(BeatPos(to_beats(file - self.origin, bpm)), lane)
    }

    /// The time in the file drawn at `x`, snapped to the grid (counted from
    /// the start marker) unless Alt is held.
    fn file_at_x(&self, ui: &Ui, x: f32, bpm: f32, lane: Rect) -> f64 {
        let beats = self.grid.x_to_beats(x, lane);
        let beats = if ui.input(|i| i.modifiers.alt) {
            beats
        } else {
            self.grid.snap_at_grid(beats)
        };
        self.origin + to_seconds(beats.0, bpm)
    }

    /// Where the whole file is drawn, left to right.
    fn file_x(&self, clip: &AudioClip, bpm: f32, lane: Rect) -> std::ops::RangeInclusive<f32> {
        self.x(0., bpm, lane)..=self.x(clip.source_seconds() as f64, bpm, lane)
    }

    /// Zoom and scroll to what the clip plays of the file: from its start
    /// marker to its end, or through its loop.
    fn fit(&mut self, clip: &AudioClip, bpm: f32, width: f32) {
        let (start, end) = played(clip, bpm);
        let margin = (end - start) * FIT_MARGIN;
        self.grid.fit(
            BeatPos(to_beats(start - self.origin, bpm) - margin),
            BeatPos(to_beats(end - self.origin, bpm) + margin),
            width,
        );
    }

    /// Ctrl+wheel or pinch zooms, like the timeline, and so does the wheel
    /// over the ruler. The wheel or a drag over the lane scrolls.
    /// Double-clicking the lane returns where to seek to: where that part of
    /// the file plays first; double-clicking the ruler fits the clip again.
    /// Also returns the ruler's response.
    fn navigate(
        &mut self,
        ui: &Ui,
        ruler: Rect,
        lane: Rect,
        clip: &AudioClip,
        bpm: f32,
    ) -> (Option<BeatPos>, Response) {
        let response = ui
            .interact(lane, ui.id().with("clip-lane"), Sense::click_and_drag())
            .on_hover_cursor(CursorIcon::Grab);
        let pointer = ui.input(|i| i.pointer.hover_pos());
        // Not through a modal or menu above.
        let over_ruler = ui.rect_contains_pointer(ruler);
        let over_lane = ui.rect_contains_pointer(lane);

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
            .on_hover_text(
                "Bars from the clip's bar 1. Double-click to fit the clip, right-click to \
                 move bar 1.",
            );
        if ruler_response.double_clicked() {
            self.fit(clip, bpm, lane.width());
        }
        let seek = response
            .double_clicked()
            .then(|| response.interact_pointer_pos())
            .flatten()
            .map(|p| clip.position + self.grid.x_to_beats(p.x, lane).0);

        let (min, max) = self.scroll_range(clip, bpm, lane);
        self.grid.offset.x = self.grid.offset.x.clamp(min, max);
        (seek, ruler_response)
    }

    /// The ruler's menu: bar 1 where it was right-clicked, at the start
    /// marker, or back at the file's start.
    fn bar_one_menu(
        &self,
        ui: &Ui,
        ruler: &Response,
        lane: Rect,
        clip: &AudioClip,
        state: &mut ProjectState,
    ) {
        let bpm = state.bpm();
        let clicked = ui.id().with("bar-one-at");
        if ruler.secondary_clicked()
            && let Some(pos) = ruler.interact_pointer_pos()
        {
            // Where the pointer is, not snapped: the grid being moved.
            let at = self.origin + to_seconds(self.grid.x_to_beats(pos.x, lane).0, bpm);
            ui.data_mut(|d| d.insert_temp(clicked, at));
        }
        ruler.context_menu(|ui| {
            let mut origin = None;
            if let Some(at) = ui.data(|d| d.get_temp::<f64>(clicked))
                && ui.button("Bar 1 here").clicked()
            {
                origin = Some(at);
            }
            if ui.button("Bar 1 at the start marker").clicked() {
                origin = Some(clip.offset);
            }
            if ui.button("Bar 1 at the file's start").clicked() {
                origin = Some(0.);
            }
            if let Some(origin) = origin {
                state.set_clip_origin(clip.id, origin);
                ui.close();
            }
        });
    }

    /// How far the view scrolls: always keeping part of the file in view.
    fn scroll_range(&self, clip: &AudioClip, bpm: f32, lane: Rect) -> (f32, f32) {
        let file_start = to_beats(-self.origin, bpm);
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

    /// Drag the markers by their flags (or the lines under them), the loop's
    /// bracket by its edges or its middle. Each edit shows while dragged
    /// and applies as one undo step on release.
    fn handles(
        &self,
        ui: &Ui,
        ruler: Rect,
        lane: Rect,
        clip: &AudioClip,
        state: &mut ProjectState,
    ) {
        let bpm = state.bpm();
        let shown = state.shown_clip(clip);
        let view = ruler.union(lane);
        let mut areas: Vec<(Handle, Rect)> = Vec::new();
        let start = self.x(shown.offset, bpm, lane);
        let marker =
            |x: f32| Rect::from_x_y_ranges(x - HANDLE_REACH..=x + HANDLE_REACH, view.y_range());
        areas.push((Handle::StartMarker, marker(start)));
        areas.push((
            Handle::StartMarker,
            flag_rect(TrimEdge::Start, start, ruler),
        ));
        match shown.active_loop() {
            Some(l) => {
                let (left, right) = (self.x(l.start_s, bpm, lane), self.x(l.end_s, bpm, lane));
                let bar = |range: std::ops::RangeInclusive<f32>| {
                    Rect::from_x_y_ranges(range, ruler.top()..=ruler.top() + LOOP_BAR_HEIGHT + 2.)
                };
                // The middle first: the edges, registered after it, win where
                // they overlap it.
                areas.push((Handle::Loop, bar(left..=right)));
                areas.push((
                    Handle::LoopStart,
                    bar(left - HANDLE_REACH..=left + HANDLE_REACH),
                ));
                areas.push((
                    Handle::LoopEnd,
                    bar(right - HANDLE_REACH..=right + HANDLE_REACH),
                ));
            }
            None => {
                let end = self.x(shown.offset + to_seconds(shown.length, bpm), bpm, lane);
                areas.push((Handle::EndMarker, marker(end)));
                areas.push((Handle::EndMarker, flag_rect(TrimEdge::End, end, ruler)));
            }
        }
        // Registered after the lane and the ruler, they win over them.
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let press = ui.input(|i| i.pointer.press_origin());
        for (i, (handle, area)) in areas.iter().enumerate() {
            let response = ui
                .interact(
                    area.intersect(view),
                    ui.id().with(("handle", i)),
                    Sense::drag(),
                )
                .on_hover_and_drag_cursor(match handle {
                    Handle::Loop => CursorIcon::Grab,
                    _ => CursorIcon::ResizeHorizontal,
                });
            if response.hovered() || response.dragged() {
                let theme = ui.app_theme();
                ui.painter_at(view)
                    .rect_filled(*area, 1., theme.hover_overlay);
            }
            if response.drag_started() {
                // The loop as it was, to move it by how far it's dragged.
                ui.data_mut(|d| d.insert_temp(response.id, shown.looped));
            }
            if response.dragged()
                && let Some(pointer) = pointer
            {
                let at = self.file_at_x(ui, pointer.x, bpm, lane);
                let moved_loop = || {
                    let from = press.map_or(pointer.x, |p| p.x);
                    let delta = self.file_at_x(ui, pointer.x, bpm, lane)
                        - self.file_at_x(ui, from, bpm, lane);
                    let grabbed: Option<ClipLoop> = ui.data(|d| d.get_temp(response.id)).flatten();
                    grabbed.map(|l| (l.start_s + delta, l.end_s + delta))
                };
                let edited = match handle {
                    Handle::StartMarker => {
                        let mut c = shown.clone();
                        c.slip_to(at, bpm);
                        Some(c)
                    }
                    Handle::EndMarker => {
                        let mut c = shown.clone();
                        let length = to_beats(at - c.offset, bpm).max(MIN_LENGTH);
                        c.trim_end_at(c.position + length, bpm);
                        Some(c)
                    }
                    Handle::LoopStart => shown
                        .active_loop()
                        .map(|l| with_loop(&shown, at, l.end_s, bpm)),
                    Handle::LoopEnd => shown
                        .active_loop()
                        .map(|l| with_loop(&shown, l.start_s, at, bpm)),
                    Handle::Loop => moved_loop().map(|(start, end)| {
                        // Kept whole inside the file.
                        let source = shown.source_seconds() as f64;
                        let shift = (-start).max(0.) - (end - source).max(0.);
                        with_loop(&shown, start + shift, end + shift, bpm)
                    }),
                };
                if let Some(edited) = edited {
                    state.resize_clip(edited);
                }
                if *handle == Handle::Loop {
                    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                }
            }
            if response.drag_stopped() {
                state.commit_resize_clip(state.shown_clip(clip));
            }
        }
    }

    /// The file's waveform on the grid, dimmed outside what the clip plays,
    /// the loop tinted, and the playhead where the file plays at that
    /// moment.
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
        if let Some(l) = clip.active_loop() {
            let (left, right) = (self.x(l.start_s, bpm, lane), self.x(l.end_s, bpm, lane));
            painter.rect_filled(
                Rect::from_x_y_ranges(left..=right, lane.y_range()),
                0.,
                with_alpha(track.color, 24),
            );
        }

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
                clip.pan,
            );
        }

        let (start, end) = played(clip, bpm);
        let (start, end) = (self.x(start, bpm, lane), self.x(end, bpm, lane));
        let dim = with_alpha(theme.bg_deep, 190);
        for part in [file.left()..=start, end..=file.right()] {
            painter.rect_filled(Rect::from_x_y_ranges(part, lane.y_range()), 0., dim);
        }
        let playhead = state.playhead();
        if clip.position <= playhead && playhead < clip.end(bpm) {
            let at = clip.file_at(playhead - clip.position, bpm);
            painter.vline(
                self.x(at, bpm, lane),
                lane.y_range(),
                Stroke::new(1., theme.playhead),
            );
        }
    }

    /// The start marker (and the end one, without a loop) across the lane
    /// with its flag in the ruler, in the text colour to stand out on the
    /// waveform; the loop's bracket along the ruler's top, in the clip's
    /// colour.
    #[allow(clippy::too_many_arguments)]
    fn paint_markers(
        &self,
        ui: &Ui,
        ruler: Rect,
        lane: Rect,
        clip: &AudioClip,
        color: Color32,
        bpm: f32,
        theme: &Theme,
    ) {
        let view = ruler.union(lane);
        let painter = ui.painter_at(view);
        if let Some(l) = clip.active_loop() {
            let (left, right) = (self.x(l.start_s, bpm, lane), self.x(l.end_s, bpm, lane));
            let bar = Rect::from_x_y_ranges(
                left..=right,
                ruler.top() + 1.0..=ruler.top() + LOOP_BAR_HEIGHT,
            );
            painter.rect_filled(bar, 1., color);
            for x in [left, right] {
                painter.vline(
                    x,
                    bar.top()..=lane.bottom(),
                    Stroke::new(1., with_alpha(color, 160)),
                );
            }
        }
        let mut markers = vec![(TrimEdge::Start, clip.offset)];
        if clip.active_loop().is_none() {
            markers.push((TrimEdge::End, clip.offset + to_seconds(clip.length, bpm)));
        }
        let ink = theme.text;
        for (edge, at) in markers {
            let x = self.x(at, bpm, lane);
            painter.vline(x, ruler.bottom()..=lane.bottom(), Stroke::new(1.5, ink));
            let flag = flag_rect(edge, x, ruler);
            let middle = egui::vec2(0., FLAG_SIZE.y / 2.);
            let (tip, top, bottom) = match edge {
                TrimEdge::Start => (
                    flag.right_top() + middle,
                    flag.left_top(),
                    flag.left_bottom(),
                ),
                TrimEdge::End => (
                    flag.left_top() + middle,
                    flag.right_top(),
                    flag.right_bottom(),
                ),
            };
            painter.add(Shape::convex_polygon(
                vec![top, tip, bottom],
                ink,
                Stroke::NONE,
            ));
            painter.vline(
                x,
                flag.y_range().min..=ruler.bottom(),
                Stroke::new(1.5, ink),
            );
        }
    }
}

/// What of the file the clip plays, in seconds: from its start marker to
/// where it ends, or through its loop.
fn played(clip: &AudioClip, bpm: f32) -> (f64, f64) {
    match clip.active_loop() {
        Some(l) => (clip.offset.min(l.start_s), l.end_s.max(clip.offset)),
        None => (clip.offset, clip.offset + to_seconds(clip.length, bpm)),
    }
}

/// `clip` looping `start..end` of its file (at least [`MIN_LENGTH`] long,
/// inside the file), its start marker no later than the loop's end.
fn with_loop(clip: &AudioClip, start: f64, end: f64, bpm: f32) -> AudioClip {
    let source = clip.source_seconds() as f64;
    let min = to_seconds(MIN_LENGTH, bpm);
    let start = start.clamp(0., (source - min).max(0.));
    let end = end.clamp(start + min, source.max(start + min));
    let mut c = clip.clone();
    c.set_loop(
        Some(ClipLoop {
            start_s: start,
            end_s: end,
        }),
        bpm,
    );
    if c.offset > end {
        c.offset = start;
    }
    c
}

/// The loop to start with: what the clip plays, or the whole file when
/// that's too short.
fn default_loop(clip: &AudioClip, bpm: f32) -> ClipLoop {
    let source = clip.source_seconds() as f64;
    let end = (clip.offset + to_seconds(clip.length, bpm)).min(source);
    if end - clip.offset >= to_seconds(MIN_LENGTH, bpm) {
        ClipLoop {
            start_s: clip.offset,
            end_s: end,
        }
    } else {
        ClipLoop {
            start_s: 0.,
            end_s: source,
        }
    }
}

fn to_beats(seconds: f64, bpm: f32) -> f64 {
    seconds * bpm as f64 / 60.
}

fn to_seconds(beats: f64, bpm: f32) -> f64 {
    beats * 60. / bpm as f64
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

/// `clip` with `edge` moved to `at`, as the timeline trims it: its sound
/// stays in place. It keeps at least [`MIN_LENGTH`].
fn trimmed(clip: &AudioClip, edge: TrimEdge, at: BeatPos, bpm: f32) -> AudioClip {
    let mut clip = clip.clone();
    match edge {
        TrimEdge::Start => clip.trim_start_at(at.min(clip.end(bpm) - MIN_LENGTH), bpm),
        TrimEdge::End => clip.trim_end_at(at.max(clip.position + MIN_LENGTH), bpm),
    }
    clip
}

/// The clip's properties, in sections as in other DAWs' inspectors: where
/// it is (start, end, length) and what it plays (offset in the file, gain,
/// pan). Positions can be dragged, or clicked and typed; edits show while
/// dragged and apply as one undo step once released (or typed). Gain and
/// pan are sliders like a track's, heard as they're dragged.
fn inspector_ui(ui: &mut Ui, clip: &AudioClip, color: Color32, state: &mut ProjectState) {
    let bpm = state.bpm();
    let bpb = state.time_signature();
    let shown = state.shown_clip(clip);
    let start = shown.position.0;
    let end = shown.end(bpm).0;
    let length = end - start;
    let source = shown.source_seconds() as f64;
    let offset = shown.offset;
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
    ui.spacing_mut().item_spacing = vec2(CELL_GAP, 1.);

    section(ui, "Clip", 0.);
    ui.columns(3, |cells| {
        let raw = cells[0].id().with("start-raw");
        let mut beats = cells[0].data(|d| d.get_temp(raw)).unwrap_or(start);
        let response = cell(
            &mut cells[0],
            "Start",
            DragValue::new(&mut beats)
                .speed(1. / 16.)
                .range(0.0..=f64::MAX)
                .custom_formatter(|v, _| format_bars(v, bpb))
                .custom_parser(|text| parse_bars(text, bpb)),
        )
        .on_hover_text(format!("{} · bar.beat.sixteenth", seconds(start)));
        keep_raw(&cells[0], raw, &response, beats);
        if let Some(done) = gesture(&response) {
            let mut moved = shown.clone();
            moved.position = BeatPos(snap(beats).max(0.));
            change = Some((moved, done));
        }

        let raw = cells[1].id().with("end-raw");
        let mut beats = cells[1].data(|d| d.get_temp(raw)).unwrap_or(end);
        let response = cell(
            &mut cells[1],
            "End",
            DragValue::new(&mut beats)
                .speed(1. / 16.)
                .range(start + MIN_LENGTH..=f64::MAX)
                .custom_formatter(|v, _| format_bars(v, bpb))
                .custom_parser(|text| parse_bars(text, bpb)),
        )
        .on_hover_text(format!("{} · bar.beat.sixteenth", seconds(end)));
        keep_raw(&cells[1], raw, &response, beats);
        if let Some(done) = gesture(&response) {
            let at = BeatPos(snap(beats).max(start + MIN_LENGTH));
            change = Some((trimmed(&shown, TrimEdge::End, at, bpm), done));
        }

        let raw = cells[2].id().with("length-raw");
        let mut beats = cells[2].data(|d| d.get_temp(raw)).unwrap_or(length);
        let response = cell(
            &mut cells[2],
            "Length",
            DragValue::new(&mut beats)
                .speed(1. / 16.)
                .range(MIN_LENGTH..=f64::MAX)
                .custom_formatter(|v, _| format_length(v, bpb))
                .custom_parser(|text| parse_length(text, bpb)),
        )
        .on_hover_text(format!("{} · bars.beats.sixteenths", seconds(length)));
        keep_raw(&cells[2], raw, &response, beats);
        if let Some(done) = gesture(&response) {
            let at = shown.position + snap(beats).max(MIN_LENGTH);
            change = Some((trimmed(&shown, TrimEdge::End, at, bpm), done));
        }
    });

    section(ui, "Audio", 0.);
    ui.columns(3, |cells| {
        let mut seconds = offset;
        let response = cell(
            &mut cells[0],
            "Offset",
            DragValue::new(&mut seconds)
                .speed(0.005)
                .range(0.0..=f64::MAX)
                .custom_formatter(|v, _| format_time(v))
                .custom_parser(parse_time),
        )
        .on_hover_text("Where in the file the clip starts playing");
        if let Some(done) = gesture(&response) {
            let mut slipped = shown.clone();
            slipped.slip_to(seconds, bpm);
            change = Some((slipped, done));
        }

        // As on tracks: a level in the clip's colour, reset by a
        // double-click; heard as it moves, one undo step once released.
        cell_label(&mut cells[1], "Gain");
        let size = vec2(cells[1].available_width(), FIELD_HEIGHT);
        let gain = ParamSlider::gain(shown.gain, CLIP_GAIN_RANGE)
            .field()
            .color(color)
            .size(size)
            .suffix(" dB");
        apply_param(state, clip.id, ClipParam::Gain, gain.show(&mut cells[1]).1);

        cell_label(&mut cells[2], "Pan");
        let (response, edit) = ParamSlider::pan(shown.pan)
            .field()
            .color(color)
            .size(size)
            .show(&mut cells[2]);
        response.on_hover_text("Balance: turning to a side lowers the other");
        apply_param(state, clip.id, ClipParam::Pan, edit);
    });

    // Its switch on the title's row.
    let looped = shown.active_loop();
    let header = section(ui, "Loop", LOOP_SWITCH_WIDTH + CELL_GAP);
    let switch = Rect::from_min_max(
        pos2(header.right() - LOOP_SWITCH_WIDTH, header.top()),
        header.max,
    );
    let toggle = FlatButton::new(if looped.is_some() { "On" } else { "Off" })
        .size(switch.size())
        .font(FontId::proportional(font_size::SMALL))
        .selected(looped.is_some())
        .tooltip("Play the loop over and over, for as long as the clip lasts");
    if ui.put(switch, toggle).clicked() {
        let mut toggled = shown.clone();
        match looped {
            Some(_) => toggled.set_loop(None, bpm),
            None => toggled.set_loop(Some(default_loop(&shown, bpm)), bpm),
        }
        change = Some((toggled, true));
    }
    if let Some(l) = looped {
        ui.columns(3, |cells| {
            let mut start = l.start_s;
            let response = cell(
                &mut cells[0],
                "Start",
                DragValue::new(&mut start)
                    .speed(0.005)
                    .range(0.0..=source)
                    .custom_formatter(|v, _| format_time(v))
                    .custom_parser(parse_time),
            )
            .on_hover_text("Where in the file the loop starts");
            if let Some(done) = gesture(&response) {
                change = Some((with_loop(&shown, start, l.end_s, bpm), done));
            }

            let raw = cells[1].id().with("loop-length-raw");
            let loop_length = to_beats(l.end_s - l.start_s, bpm);
            let mut beats = cells[1].data(|d| d.get_temp(raw)).unwrap_or(loop_length);
            let response = cell(
                &mut cells[1],
                "Length",
                DragValue::new(&mut beats)
                    .speed(1. / 16.)
                    .range(MIN_LENGTH..=f64::MAX)
                    .custom_formatter(|v, _| format_length(v, bpb))
                    .custom_parser(|text| parse_length(text, bpb)),
            )
            .on_hover_text(format!("{} · bars.beats.sixteenths", seconds(loop_length)));
            keep_raw(&cells[1], raw, &response, beats);
            if let Some(done) = gesture(&response) {
                let end = l.start_s + to_seconds(snap(beats).max(MIN_LENGTH), bpm);
                change = Some((with_loop(&shown, l.start_s, end, bpm), done));
            }
        });
    }

    match change {
        Some((edited, false)) => state.resize_clip(edited),
        Some((edited, true)) => state.commit_resize_clip(edited),
        None => {}
    }
}

/// A section's title, small and muted, with a rule after it up to
/// `reserve` from the right (for something on its row). Returns its row.
fn section(ui: &mut Ui, title: &str, reserve: f32) -> Rect {
    ui.add_space(SECTION_GAP);
    let theme = ui.app_theme();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.), Sense::hover());
    let painter = ui.painter();
    let text = painter.text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        title.to_uppercase(),
        semibold(font_size::SMALL),
        theme.text_muted,
    );
    let rule_end = rect.right() - reserve;
    if text.right() + 6. < rule_end {
        painter.hline(
            text.right() + 6.0..=rule_end,
            rect.center().y,
            Stroke::new(1., theme.separator),
        );
    }
    ui.add_space(2.);
    rect
}

/// A cell's label, small and muted, above its value.
fn cell_label(ui: &mut Ui, text: &str) {
    ui.add(
        Label::new(
            RichText::new(text)
                .size(font_size::SMALL)
                .color(ui.app_theme().text_muted),
        )
        .truncate(),
    );
}

/// A cell: its label above a value field filling its width.
fn cell(ui: &mut Ui, label: &str, value: DragValue) -> egui::Response {
    cell_label(ui, label);
    let width = ui.available_width();
    ui.add_sized(vec2(width, FIELD_HEIGHT), value)
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

/// A length as typed, `bars.beats.sixteenths` (`1.2.0`; missing parts are
/// none), in beats.
fn parse_length(text: &str, signature: TimeSignature) -> Option<f64> {
    let parts = parse_parts(text)?;
    let part = |i: usize| parts.get(i).copied().unwrap_or(0.);
    if parts.is_empty() || parts.iter().any(|p| *p < 0.) {
        return None;
    }
    Some(part(0) * signature.beats_per_bar() + part(1) * signature.beat_length() + part(2) / 4.)
}

/// A length as `bars.beats.sixteenths`, counting from 0, beats as
/// `signature` counts them: one bar and two beats is `1.2.0`.
fn format_length(beats: f64, signature: TimeSignature) -> String {
    let sixteenths = (beats * 4.).round().max(0.) as u64;
    // Whole numbers of sixteenths, whatever the signature.
    let per_bar = ((signature.beats_per_bar() * 4.).round() as u64).max(1);
    let per_beat = ((signature.beat_length() * 4.).round() as u64).max(1);
    let within = sixteenths % per_bar;
    format!(
        "{}.{}.{}",
        sixteenths / per_bar,
        within / per_beat,
        within % per_beat
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::AudioInfo;

    /// A clip at beat 10 playing the middle half of a 4 s (8-beat, at 120
    /// bpm) file: from 1 s, for 4 beats.
    fn clip() -> AudioClip {
        let mut clip = AudioClip::new(ClipId(1), AudioInfo::silent(4.), BeatPos(10.), 120.);
        clip.offset = 1.;
        clip.length = 4.;
        clip
    }

    #[test]
    fn trimming_the_start_keeps_the_audio_in_place() {
        let t = trimmed(&clip(), TrimEdge::Start, BeatPos(9.), 120.);
        assert_eq!((t.offset, t.position, t.length), (0.5, BeatPos(9.), 5.));
        // No further than the file's start...
        let t = trimmed(&clip(), TrimEdge::Start, BeatPos(5.), 120.);
        assert_eq!((t.offset, t.position), (0., BeatPos(8.)));
        // ...nor past the end, minus the minimum length.
        let t = trimmed(&clip(), TrimEdge::Start, BeatPos(15.), 120.);
        assert_eq!(t.length, MIN_LENGTH);
    }

    #[test]
    fn trimming_the_start_stops_at_the_arrangement_start() {
        let mut near_zero = clip();
        near_zero.position = BeatPos(1.); // the file would start at beat -1
        let t = trimmed(&near_zero, TrimEdge::Start, BeatPos(-1.), 120.);
        assert_eq!((t.offset, t.position), (0.5, BeatPos::ZERO));
    }

    #[test]
    fn trimming_the_end() {
        let t = trimmed(&clip(), TrimEdge::End, BeatPos(15.), 120.);
        assert_eq!((t.length, t.position, t.offset), (5., BeatPos(10.), 1.));
        // To the file's end, no further: 3 s left, 6 beats.
        assert_eq!(
            trimmed(&clip(), TrimEdge::End, BeatPos(28.), 120.).length,
            6.
        );
        assert_eq!(
            trimmed(&clip(), TrimEdge::End, BeatPos(8.), 120.).length,
            MIN_LENGTH
        );
    }

    #[test]
    fn loops_stay_in_the_file_and_start_as_what_plays() {
        let looped = default_loop(&clip(), 120.);
        assert_eq!((looped.start_s, looped.end_s), (1., 3.));
        let c = with_loop(&clip(), -1., 10., 120.);
        let l = c.looped.unwrap();
        assert_eq!((l.start_s, l.end_s), (0., 4.), "inside the file");
        // Moving the loop before the start marker brings the marker in.
        let c = with_loop(&clip(), 0., 0.5, 120.);
        assert_eq!(c.offset, 0.);
        // Never shorter than the minimum.
        let l = with_loop(&clip(), 2., 2., 120.).looped.unwrap();
        assert!(l.end_s > l.start_s);
    }

    #[test]
    fn typed_positions_and_lengths_read_as_shown() {
        const FOUR: TimeSignature = TimeSignature {
            numerator: 4,
            denominator: 4,
        };
        assert_eq!(parse_bars("3.2.3", FOUR), Some(9.5));
        assert_eq!(parse_bars("1.1.1", FOUR), Some(0.));
        assert_eq!(
            parse_bars("2", FOUR),
            Some(4.),
            "missing parts are the first"
        );
        assert_eq!(parse_bars("0.1.1", FOUR), None, "bars count from 1");
        assert_eq!(parse_bars("inf", FOUR), None);
        assert_eq!(parse_bars("1.NaN", FOUR), None);
        assert_eq!(parse_bars("1.2.3.4", FOUR), None);
        assert_eq!(parse_bars("x", FOUR), None);
        assert_eq!(parse_length("1.2.1", FOUR), Some(6.25));
        assert_eq!(parse_length("3", FOUR), Some(12.), "bars first");
        assert_eq!(parse_length("0.0.2", FOUR), Some(0.5));
        for beats in [0., 6.25, 9.5] {
            assert_eq!(parse_bars(&format_bars(beats, FOUR), FOUR), Some(beats));
        }
    }

    #[test]
    fn lengths_count_bars_beats_and_sixteenths_from_zero() {
        const FOUR: TimeSignature = TimeSignature {
            numerator: 4,
            denominator: 4,
        };
        assert_eq!(format_length(4., FOUR), "1.0.0");
        assert_eq!(format_length(6.25, FOUR), "1.2.1");
        assert_eq!(format_length(3., FOUR), "0.3.0");
        for beats in [0.25, 6.25, 12.] {
            assert_eq!(parse_length(&format_length(beats, FOUR), FOUR), Some(beats));
        }
        // 6/8 counts eighths: a bar is 3 beats, of 6 eighths.
        let six_eight = TimeSignature::new(6, 8).unwrap();
        assert_eq!(format_length(3.5, six_eight), "1.1.0");
        assert_eq!(parse_length("1.1.0", six_eight), Some(3.5));
        assert_eq!(parse_bars("2.4.2", six_eight), Some(4.75))
    }

    #[test]
    fn typed_times() {
        assert_eq!(parse_time("1:23.25"), Some(83.25));
        assert_eq!(parse_time("2.5"), Some(2.5));
        assert_eq!(parse_time("-1"), None);
    }

    /// Drives the view of one clip headless, with real pointer events.
    struct Harness {
        ctx: egui::Context,
        details: ClipDetails,
        state: ProjectState,
        clip: ClipId,
    }

    impl Harness {
        /// A 2 s (4-beat) file played from 0.5 s for 2 beats, at beat 5.
        fn new() -> Self {
            use tonique_engine::engine::{Engine, EngineConfig};
            let (engine, _processor) = Engine::new(EngineConfig::default());
            let mut state = ProjectState::new(engine);
            let track = state.add_track();
            let clip = state.new_clip_id();
            let mut audio = AudioClip::new(clip, AudioInfo::silent(2.), BeatPos(5.), 120.);
            audio.offset = 0.5;
            audio.length = 2.;
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

        /// The middle of the lane, `at` seconds into the file.
        fn at(&self, at: f64) -> egui::Pos2 {
            let lane = self.details.lane;
            egui::pos2(self.details.x(at, 120., lane), lane.center().y)
        }

        /// The loop's bracket, at the ruler's top, `at` seconds in.
        fn on_loop(&self, at: f64) -> egui::Pos2 {
            let lane = self.details.lane;
            let top = lane.top() - RULER_HEIGHT + LOOP_BAR_HEIGHT / 2.;
            egui::pos2(self.details.x(at, 120., lane), top)
        }
    }

    #[test]
    fn dragging_the_end_marker_trims_the_clip_in_one_step() {
        let mut h = Harness::new();
        assert_eq!(h.clip().end(120.), BeatPos(7.)); // at 1.5 s in the file
        // Out past the file's end: it stops there, at 2 s.
        let (from, to) = (h.at(1.5), h.at(2.5));
        h.drag(from, to);
        assert_eq!(h.clip().length, 3.);
        h.state.undo();
        assert_eq!(h.clip().length, 2., "one step");
    }

    #[test]
    fn the_start_marker_slips_the_audio_and_bar_one_stays() {
        let mut h = Harness::new();
        let file_start = h.at(0.).x;
        assert_eq!(h.details.origin, 0., "the file's start");
        let (from, to) = (h.at(0.5), h.at(0.75));
        h.drag(from, to);
        let clip = h.clip();
        assert_eq!((clip.offset, clip.position), (0.75, BeatPos(5.)), "slipped");
        assert_eq!(h.details.origin, 0., "bar 1 stayed");
        assert!((h.at(0.).x - file_start).abs() < 0.01, "the audio stayed");
    }

    #[test]
    fn moving_bar_one_keeps_the_audio_in_place_and_is_undone() {
        let mut h = Harness::new();
        let file_start = h.at(0.).x;
        h.state.set_clip_origin(h.clip, 0.25);
        h.frame(Vec::new());
        assert_eq!(h.details.origin, 0.25);
        assert!((h.at(0.).x - file_start).abs() < 0.01, "the audio stayed");
        h.state.undo();
        h.frame(Vec::new());
        assert_eq!(h.details.origin, 0.);
    }

    #[test]
    fn the_loop_brackets_end_drags_and_the_clip_outgrows_the_file() {
        let mut h = Harness::new();
        let mut looped = h.clip();
        looped.set_loop(
            Some(ClipLoop {
                start_s: 0.5,
                end_s: 1.5,
            }),
            120.,
        );
        looped.trim_end_at(BeatPos(20.), 120.);
        h.state.commit_resize_clip(looped);
        assert_eq!(h.clip().end(120.), BeatPos(20.), "past the file's end");
        h.frame(Vec::new());

        let (from, to) = (h.on_loop(1.5), h.on_loop(1.75));
        h.drag(from, to);
        let l = h.clip().looped.unwrap();
        assert_eq!((l.start_s, l.end_s), (0.5, 1.75));
        h.state.undo();
        assert_eq!(h.clip().looped.unwrap().end_s, 1.5, "one step");
    }
}
