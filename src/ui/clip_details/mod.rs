//! The bottom panel's view of one clip.

mod audio;
mod fades;
mod inspector;
mod markers;
#[cfg(test)]
mod tests;

use egui::{CursorIcon, Rect, Response, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use tonique_engine::{edit::ClipId, time::BeatPos};

use crate::{
    core::{clip::AudioClip, grid::GridService, state::ProjectState, track::TrackRow},
    ui::{
        arrangement::grid::PaintGrid,
        theme::{Theme, ThemeExt, with_alpha},
        waveform::paint_waveform,
        widget::scroll_bar::{Axis, SCROLLBAR_WIDTH, ScrollBar},
    },
};

use inspector::{Panels, RAIL_WIDTH, inspector_ui};
use markers::FLAG_SIZE;

const PADDING: f32 = 8.;
/// The least the waveform's view is given, the inspector taking the rest.
const MIN_VIEW_WIDTH: f32 = 240.;
/// The ruler: the loop's bracket along its top, the bars, and the markers'
/// flags along its bottom.
const RULER_HEIGHT: f32 = 24.;
const LOOP_BAR_HEIGHT: f32 = 6.;
/// Between the lane and its scrollbar.
const SCROLLBAR_GAP: f32 = 3.;
/// Room left around the clip when fitting it, as a share of its length.
const FIT_MARGIN: f64 = 0.02;

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
    /// Seconds of the file the clip plays per second, as last drawn: the
    /// grid counts beats as heard.
    speed: f64,
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
            speed: 1.,
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
        // The waveform gives way to the panels, down to its least; then the
        // panels scroll.
        let inspector_width = Panels::get(ui.ctx())
            .width()
            .min(rect.width() - PADDING - MIN_VIEW_WIDTH)
            .max(RAIL_WIDTH);
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
        // Played faster or slower: the file spans fewer or more beats, the
        // view kept as it was (its zoom and scroll, in beats).
        self.speed = clip.speed(bpm);
        if self.fitted == Some(clip.id) && clip.origin != self.origin {
            // Bar 1 moved: the grid follows, and the audio stays where it
            // was on screen.
            self.grid.offset.x -= self
                .grid
                .beats_to_width(clip.file_beats(clip.origin - self.origin, bpm));
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
            inspector_ui(ui, clip, track.color, state)
        });
        ui.allocate_rect(rect, Sense::hover());
    }

    /// Beats the clip takes to play `seconds` of the file.
    fn beats(&self, seconds: f64, bpm: f32) -> f64 {
        to_beats(seconds / self.speed, bpm)
    }

    /// Seconds of the file the clip plays in `beats`.
    fn seconds(&self, beats: f64, bpm: f32) -> f64 {
        to_seconds(beats, bpm) * self.speed
    }

    /// Where `file` seconds into the file are drawn.
    fn x(&self, file: f64, bpm: f32, lane: Rect) -> f32 {
        self.grid
            .beats_to_x(BeatPos(self.beats(file - self.origin, bpm)), lane)
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
        self.origin + self.seconds(beats.0, bpm)
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
            BeatPos(clip.file_beats(start - self.origin, bpm) - margin),
            BeatPos(clip.file_beats(end - self.origin, bpm) + margin),
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
        let ruler_response = ui.interact(ruler, ui.id().with("clip-ruler"), Sense::click());
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
            let at = self.origin + self.seconds(self.grid.x_to_beats(pos.x, lane).0, bpm);
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
        let file_start = clip.file_beats(-self.origin, bpm);
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
}

/// What of the file the clip plays, in seconds: from its start marker to
/// where it ends, or through its loop.
fn played(clip: &AudioClip, bpm: f32) -> (f64, f64) {
    match clip.active_loop() {
        Some(l) => (clip.offset.min(l.start_s), l.end_s.max(clip.offset)),
        None => (
            clip.offset,
            clip.offset + clip.file_seconds(clip.length, bpm),
        ),
    }
}

fn to_beats(seconds: f64, bpm: f32) -> f64 {
    seconds * bpm as f64 / 60.
}

fn to_seconds(beats: f64, bpm: f32) -> f64 {
    beats * 60. / bpm as f64
}
