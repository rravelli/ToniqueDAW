//! The panel's view of one clip, as the egui app's `ClipDetails`: an
//! inspector of its properties on the left ([`inspector`]), and on the
//! right its whole source file on a grid counted from the clip's bar 1
//! (the file's start unless moved), zoomed and scrolled. Markers in the
//! ruler show where playback starts and (without a loop) ends, and a
//! bracket the loop; all are dragged ([`markers`]).
//!
//! Positions across the view are local to it, from its left edge: the
//! canvas adds where it's laid out.

mod arrows;
mod audio;
mod inspector;
mod markers;

pub use arrows::ClipArrows;

use crate::bottom::BottomPanel;
use crate::model::HoldsProject;
use std::{cell::Cell, rc::Rc};

use gpui::{
    Anchor, AnyElement, App, Bounds, Context, CursorStyle, MouseButton, MouseDownEvent,
    MouseMoveEvent, Pixels, ScrollEvent, Window, canvas, div, prelude::*, px,
};
use tonique_engine::{edit::ClipId, time::BeatPos};

use crate::{
    app::PopupKind,
    bottom::paint::{Align, fill, fill_rounded, text},
    geometry::Rect,
    smooth_scroll::WheelDelta,
    theme::{Color, Theme, hsla, mono, size, with_alpha},
    wave::paint_waveform,
    widgets::{
        Axis, Menu, MenuItem, SCROLLBAR_WIDTH, ScrollSpan, ScrollbarGrab, SliderDrag, scrollbar,
    },
};
use inspector::{Field, Panels, RAIL_WIDTH};
use markers::{Area, Handle, TrimEdge};
use tonique_core::core::{
    clip::AudioClip,
    grid::{GridLine, GridService, Span},
    track::TrackRow,
};

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
/// How dim what the clip doesn't play is.
const DIM_ALPHA: u8 = 190;
const LOOP_TINT_ALPHA: u8 = 24;

/// What a drag in the view or the inspector moves.
enum ClipDrag {
    /// The lane, scrolling the view.
    Pan {
        last_x: f32,
    },
    Handle(markers::HandleDrag),
    /// An inspector's value, dragged sideways from `start`.
    Field {
        field: Field,
        start_x: f32,
        start: f64,
        moved: bool,
    },
    /// A gain, pan or pitch slider.
    Slider {
        slider: audio::ClipSlider,
        drag: SliderDrag,
        bounds: Bounds<Pixels>,
    },
}

pub struct ClipDetails {
    /// The view's zoom and scroll, in beats from `origin`.
    grid: GridService,
    /// The clip the view was fitted to: another one refits it.
    fitted: Option<ClipId>,
    clip: Option<ClipId>,
    /// Where bar 1 is in the file, in seconds: the clip's, as last drawn.
    origin: f64,
    /// Seconds of the file the clip plays per second, as last drawn.
    speed: f64,
    /// The view (ruler, lane, scrollbar), as last laid out.
    view: Rc<Cell<Bounds<Pixels>>>,
    drag: Option<ClipDrag>,
    scrollbar: ScrollbarGrab,
    panels: Panels,
    /// A value being typed, and its text so far.
    typing: Option<(inspector::Typed, crate::widgets::TextField)>,
}

/// The view's parts, top to bottom, `width` wide, local to it.
#[derive(Clone, Copy)]
struct Layout {
    width: f32,
    /// The lane's top and bottom; the ruler is above it.
    lane: (f32, f32),
}

impl Layout {
    fn of(height: f32, width: f32) -> Self {
        // The scrollbar along the bottom.
        let scrollbar = (height - SCROLLBAR_WIDTH).max(RULER_HEIGHT);
        Self {
            width,
            lane: (RULER_HEIGHT, (scrollbar - SCROLLBAR_GAP).max(RULER_HEIGHT)),
        }
    }

    fn span(&self) -> Span {
        Span {
            left: 0.,
            right: self.width,
        }
    }
}

impl ClipDetails {
    pub fn new() -> Self {
        Self {
            // Shows what's before the start marker, in the file.
            grid: GridService::new().with_negative_offset(),
            fitted: None,
            clip: None,
            origin: 0.,
            speed: 1.,
            view: Rc::default(),
            drag: None,
            scrollbar: ScrollbarGrab::default(),
            panels: Panels::default(),
            typing: None,
        }
    }

    pub fn forget(&mut self) {
        self.fitted = None;
        self.clip = None;
        self.drag = None;
        self.typing = None;
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    fn layout(&self) -> Layout {
        let view = self.view.get();
        Layout::of(f32::from(view.size.height), f32::from(view.size.width))
    }

    /// The pointer at `position`, local to the view.
    fn local(&self, position: gpui::Point<Pixels>) -> (f32, f32) {
        let view = self.view.get();
        (
            f32::from(position.x - view.origin.x),
            f32::from(position.y - view.origin.y),
        )
    }

    /// Beats the clip takes to play `seconds` of the file.
    fn beats(&self, seconds: f64, bpm: f32) -> f64 {
        seconds / self.speed * bpm as f64 / 60.
    }

    /// Seconds of the file the clip plays in `beats`.
    fn seconds(&self, beats: f64, bpm: f32) -> f64 {
        beats * 60. / bpm as f64 * self.speed
    }

    /// Where `file` seconds into the file are drawn.
    fn x(&self, file: f64, bpm: f32, span: Span) -> f32 {
        self.grid
            .beats_to_x(BeatPos(self.beats(file - self.origin, bpm)), span)
    }

    /// The time in the file drawn at `x`, snapped to the grid when
    /// snapping, which Alt inverts.
    fn file_at_x(&self, x: f32, bpm: f32, span: Span, alt: bool) -> f64 {
        let beats = self.grid.snap(self.grid.x_to_beats(x, span), alt);
        self.origin + self.seconds(beats.0, bpm)
    }

    /// Where the whole file is drawn, left to right.
    fn file_x(&self, clip: &AudioClip, bpm: f32, span: Span) -> (f32, f32) {
        (
            self.x(0., bpm, span),
            self.x(clip.source_seconds() as f64, bpm, span),
        )
    }

    /// Zoom and scroll to what the clip plays of the file.
    fn fit(&mut self, clip: &AudioClip, bpm: f32, width: f32) {
        let (start, end) = played(clip, bpm);
        let margin = (end - start) * FIT_MARGIN;
        self.grid.fit(
            BeatPos(clip.file_beats(start - self.origin, bpm) - margin),
            BeatPos(clip.file_beats(end - self.origin, bpm) + margin),
            width,
        );
    }

    /// How far the view scrolls: always keeping part of the file in view.
    fn scroll_range(&self, clip: &AudioClip, bpm: f32, width: f32) -> (f32, f32) {
        let file_start = clip.file_beats(-self.origin, bpm);
        let half = width / 2.;
        let min = self.grid.beats_to_width(file_start) - half;
        let max = self
            .grid
            .beats_to_width(file_start + clip.source_beats(bpm))
            - half;
        (min, max.max(min))
    }

    fn clamp_scroll(&mut self, clip: &AudioClip, bpm: f32, width: f32) {
        let (min, max) = self.scroll_range(clip, bpm, width);
        self.grid.offset.x = self.grid.offset.x.clamp(min, max);
    }

    /// The scroll for the scrollbar, counted from the range's start.
    fn scroll_span(&self, clip: &AudioClip, bpm: f32, width: f32) -> ScrollSpan {
        let (min, max) = self.scroll_range(clip, bpm, width);
        ScrollSpan {
            offset: self.grid.offset.x - min,
            max: max - min,
            view: width,
        }
    }

    /// The grid lines and the bar labels in view, inside the file.
    fn marks(&self, clip: &AudioClip, bpm: f32, span: Span) -> Marks {
        let (from, to) = self.grid.visible_beats(span);
        let (file_left, file_right) = self.file_x(clip, bpm, span);
        let inside = |x: f32| x >= file_left - 0.5 && x <= file_right + 0.5;
        let lines = self
            .grid
            .lines(from, to, self.grid.line_step())
            .into_iter()
            .map(|(beat, line)| (self.grid.beats_to_x(beat, span), is_bar(&line)))
            .filter(|(x, _)| inside(*x))
            .collect();
        let labels = self
            .grid
            .lines(from, to, self.grid.label_step(from, to))
            .into_iter()
            .map(|(beat, line)| (self.grid.beats_to_x(beat, span), label(line), is_bar(&line)))
            .filter(|(x, ..)| inside(*x))
            .collect();
        Marks { lines, labels }
    }
}

fn is_bar(line: &GridLine) -> bool {
    matches!(line, GridLine::Bar(_))
}

fn label(line: GridLine) -> String {
    match line {
        GridLine::Bar(number) => format!("{number}"),
        GridLine::Beat { bar, beat } => format!("{bar}.{beat}"),
        GridLine::Division {
            bar,
            beat,
            sub: Some(division),
        } => format!("{bar}.{beat}.{division}"),
        GridLine::Division { bar, beat, .. } => format!("{bar}.{beat}"),
    }
}

/// What of the file the clip plays, in seconds: from its start marker to
/// where it ends, or through its loop.
fn played(clip: &AudioClip, bpm: f32) -> (f64, f64) {
    match clip.active_loop() {
        Some(looped) => (
            clip.offset.min(looped.start_s),
            looped.end_s.max(clip.offset),
        ),
        None => (
            clip.offset,
            clip.offset + clip.file_seconds(clip.length, bpm),
        ),
    }
}

struct Marks {
    lines: Vec<(f32, bool)>,
    labels: Vec<(f32, String, bool)>,
}

/// What the view's canvas paints, local to the view.
struct ClipScene {
    theme: std::rc::Rc<Theme>,
    color: Color,
    clip: AudioClip,
    layout: Layout,
    file: (f32, f32),
    marks: Marks,
    loop_x: Option<(f32, f32)>,
    played: (f32, f32),
    playhead: Option<f32>,
    markers: Vec<(TrimEdge, f32)>,
    /// The handle under the pointer, or dragged.
    lit: Option<Area>,
}

impl BottomPanel {
    pub(super) fn clip_details_view(
        &mut self,
        clip: &AudioClip,
        track: &TrackRow,
        window: &mut Window,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let state = self.state(context);
        let (bpm, time_signature, grid_settings) =
            (state.bpm(), state.time_signature(), state.grid.settings);
        let details = &mut self.clip;
        if details.clip != Some(clip.id) {
            details.typing = None;
            details.clip = Some(clip.id);
        }
        details.grid.set_time_signature(time_signature);
        // The arrangement's grid settings, at this view's zoom.
        details.grid.settings = grid_settings;
        // Played faster or slower: the file spans fewer or more beats, the
        // view kept as it was.
        details.speed = clip.speed(bpm);
        if details.fitted == Some(clip.id) && clip.origin != details.origin {
            // Bar 1 moved: the grid follows, the audio stays on screen.
            details.grid.offset.x -= details
                .grid
                .beats_to_width(clip.file_beats(clip.origin - details.origin, bpm));
        }
        details.origin = clip.origin;
        let layout = details.layout();
        if details.fitted != Some(clip.id) {
            if layout.width > 0. {
                details.fit(clip, bpm, layout.width);
                details.fitted = Some(clip.id);
            } else {
                // Not laid out yet: next frame.
                window.request_animation_frame();
            }
        }
        details.clamp_scroll(clip, bpm, layout.width);

        // As being dragged, here or in the timeline.
        let shown = self.state(context).shown_clip(clip);
        let scene = self.clip_scene(
            &shown,
            track.color,
            layout,
            crate::theme::current(context),
            window,
            context,
        );
        let cursor = self.clip_cursor(scene.lit.as_ref(), window);
        let waves = self.waves.clone();
        let slot = self.clip.view.clone();
        let painting = canvas(
            move |bounds, _, _| slot.set(bounds),
            move |bounds, _, window, context| {
                window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                    paint_view(
                        &scene,
                        Rect::of(bounds),
                        window,
                        context,
                        &mut waves.borrow_mut(),
                    )
                })
            },
        )
        .size_full();
        let id = clip.id;
        let span = self.clip.scroll_span(clip, bpm, layout.width);
        let (min, _) = self.clip.scroll_range(clip, bpm, layout.width);
        let this = context.weak_entity();
        let across_bar = scrollbar(
            Axis::Horizontal,
            &self.clip.scrollbar,
            &crate::theme::current(context),
            move |_| Some(span),
            move |offset, _, app| {
                let _ = this.update(app, |this, context| {
                    this.clip.grid.offset.x = min + offset;
                    context.notify();
                });
            },
        )
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .h(px(SCROLLBAR_WIDTH));
        let view = div()
            .id("clip-view")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .cursor(cursor)
            .child(painting)
            .child(across_bar)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, event: &MouseDownEvent, _, context| {
                    this.clip_view_press(id, event, context);
                    context.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                context.listener(move |this, event: &MouseDownEvent, _, context| {
                    this.clip_ruler_menu(id, event, context);
                    context.notify();
                }),
            )
            .on_scroll(
                context.listener(move |this, event: &ScrollEvent, _, context| {
                    this.clip_view_scroll(id, event, context);
                    context.notify();
                }),
            );

        // The waveform gives way to the inspector, down to its least; then
        // the inspector's panels scroll.
        let room = f32::from(self.bounds.get().size.width) - 3. * PADDING;
        let inspector_width = self
            .clip
            .panels
            .width()
            .min(room - MIN_VIEW_WIDTH)
            .max(RAIL_WIDTH);
        let inspector = self.inspector_view(clip, track.color, inspector_width, context);
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .gap(px(PADDING))
            .p(px(PADDING))
            .child(inspector)
            .child(view)
            .into_any_element()
    }

    fn clip_scene(
        &self,
        shown: &AudioClip,
        color: Color,
        layout: Layout,
        theme: std::rc::Rc<Theme>,
        window: &Window,
        context: &App,
    ) -> ClipScene {
        let details = &self.clip;
        let bpm = self.state(context).bpm();
        let span = layout.span();
        let x = |file: f64| details.x(file, bpm, span);
        let (start, end) = played(shown, bpm);
        let playhead = self.state(context).playhead();
        let playhead = (shown.position <= playhead && playhead < shown.end(bpm))
            .then(|| x(shown.file_at(playhead - shown.position, bpm)));
        let mut markers = vec![(TrimEdge::Start, x(shown.offset))];
        if shown.active_loop().is_none() {
            markers.push((
                TrimEdge::End,
                x(shown.offset + shown.file_seconds(shown.length, bpm)),
            ));
        }
        let lit = match &details.drag {
            Some(ClipDrag::Handle(drag)) => Some(drag.area),
            _ => {
                let (pointer_x, pointer_y) = details.local(window.mouse_position());
                self.handle_at(shown, layout, pointer_x, pointer_y, context)
            }
        };
        ClipScene {
            theme,
            color,
            clip: shown.clone(),
            layout,
            file: details.file_x(shown, bpm, span),
            marks: details.marks(shown, bpm, span),
            loop_x: shown
                .active_loop()
                .map(|looped| (x(looped.start_s), x(looped.end_s))),
            played: (x(start), x(end)),
            playhead,
            markers,
            lit,
        }
    }

    fn clip_cursor(&self, lit: Option<&Area>, window: &Window) -> CursorStyle {
        let details = &self.clip;
        match &details.drag {
            Some(ClipDrag::Pan { .. }) => return CursorStyle::ClosedHand,
            Some(ClipDrag::Handle(drag)) if drag.area.handle == Handle::Loop => {
                return CursorStyle::ClosedHand;
            }
            Some(ClipDrag::Handle(_)) => return CursorStyle::ResizeLeftRight,
            _ => {}
        }
        match lit.map(|area| area.handle) {
            Some(Handle::Loop) => CursorStyle::OpenHand,
            Some(_) => CursorStyle::ResizeLeftRight,
            None => {
                let layout = details.layout();
                let (_, pointer_y) = details.local(window.mouse_position());
                if pointer_y >= layout.lane.0 && pointer_y < layout.lane.1 {
                    CursorStyle::OpenHand
                } else {
                    CursorStyle::Arrow
                }
            }
        }
    }

    /// A press in the view: a marker or the loop grabbed, the lane
    /// panned (or, double-clicked, the playhead sent where that part of the
    /// file plays first), or the ruler double-clicked to fit the clip
    /// again.
    fn clip_view_press(&mut self, id: ClipId, event: &MouseDownEvent, context: &mut App) {
        let Some((_, clip)) = self.state(context).clip_with_track(id) else {
            return;
        };
        let shown = self.state(context).shown_clip(&clip);
        let bpm = self.state(context).bpm();
        let details = &self.clip;
        let layout = details.layout();
        let (pointer_x, pointer_y) = details.local(event.position);
        if let Some(area) = self.handle_at(&shown, layout, pointer_x, pointer_y, context) {
            self.clip.drag = Some(ClipDrag::Handle(markers::HandleDrag {
                area,
                press_x: pointer_x,
                grabbed: shown.looped,
            }));
            return;
        }
        let details = &mut self.clip;
        if pointer_y < layout.lane.0 {
            if event.click_count >= 2 {
                details.fit(&clip, bpm, layout.width);
            }
        } else if pointer_y < layout.lane.1 {
            if event.click_count >= 2 {
                let beats = details.grid.x_to_beats(pointer_x, layout.span());
                self.state_mut(context).seek(clip.position + beats.0);
            } else {
                details.drag = Some(ClipDrag::Pan { last_x: pointer_x });
            }
        }
    }

    /// A right-click on the ruler: where to put bar 1.
    fn clip_ruler_menu(&mut self, id: ClipId, event: &MouseDownEvent, context: &mut Context<Self>) {
        let details = &self.clip;
        let (pointer_x, pointer_y) = details.local(event.position);
        if pointer_y >= RULER_HEIGHT {
            return;
        }
        // Where the pointer is, not snapped: the grid being moved.
        let beats = details.grid.x_to_beats(pointer_x, details.layout().span());
        let at = details.origin + details.seconds(beats.0, self.state(context).bpm());
        self.toggle_popup(
            PopupKind::ClipRuler(id, at),
            event.position,
            Anchor::TopLeft,
            context,
        );
    }

    /// Ctrl+wheel zooms, like the timeline, and so does the wheel over the
    /// ruler; elsewhere it scrolls across.
    fn clip_view_scroll(&mut self, id: ClipId, event: &ScrollEvent, context: &mut App) {
        let Some((_, clip)) = self.state(context).clip_with_track(id) else {
            return;
        };
        let bpm = self.state(context).bpm();
        let wheel = WheelDelta::of(event.delta);
        let details = &mut self.clip;
        let layout = details.layout();
        let (pointer_x, pointer_y) = details.local(event.position);
        let command = event.modifiers.control || event.modifiers.platform;
        if command || pointer_y < RULER_HEIGHT {
            details
                .grid
                .zoom_around(-wheel.vertical, pointer_x, layout.span());
        } else {
            // Only across here: the vertical wheel too.
            details.grid.offset.x -= if wheel.horizontal != 0. {
                wheel.horizontal
            } else {
                wheel.vertical
            };
        }
        details.clamp_scroll(&clip, bpm, layout.width);
    }

    /// The ruler's menu: bar 1 where it was right-clicked (`at`, in the
    /// file), at the start marker, or back at the file's start.
    pub fn clip_ruler_popup(
        &self,
        id: ClipId,
        at: f64,
        context: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let (_, clip) = self.state(context).clip_with_track(id)?;
        let offset = clip.offset;
        Some(
            Menu::new(&crate::theme::current(context))
                .item(
                    MenuItem::new("Bar 1 here"),
                    context,
                    move |this, _, context| this.state_mut(context).set_clip_origin(id, at),
                )
                .item(
                    MenuItem::new("Bar 1 at the start marker"),
                    context,
                    move |this, _, context| this.state_mut(context).set_clip_origin(id, offset),
                )
                .item(
                    MenuItem::new("Bar 1 at the file's start"),
                    context,
                    move |this, _, context| this.state_mut(context).set_clip_origin(id, 0.),
                )
                .into_element(),
        )
    }

    pub(super) fn clip_drag_move(&mut self, event: &MouseMoveEvent, context: &mut App) -> bool {
        let Some(id) = self.clip.clip else {
            return false;
        };
        if self.clip.drag.is_none() {
            return false;
        }
        let Some((_, clip)) = self.state(context).clip_with_track(id) else {
            self.clip.drag = None;
            return false;
        };
        let bpm = self.state(context).bpm();
        let (pointer_x, _) = self.clip.local(event.position);
        let details = &mut self.clip;
        let layout = details.layout();
        match details.drag.as_mut() {
            Some(ClipDrag::Pan { last_x }) => {
                details.grid.offset.x -= pointer_x - *last_x;
                *last_x = pointer_x;
                details.clamp_scroll(&clip, bpm, layout.width);
            }
            Some(ClipDrag::Handle(_)) => {
                self.follow_handle(&clip, pointer_x, event.modifiers.alt, context)
            }
            Some(ClipDrag::Field { .. }) => self.follow_field(
                &clip,
                f32::from(event.position.x),
                event.modifiers.alt,
                context,
            ),
            Some(ClipDrag::Slider { .. }) => self.follow_slider(
                &clip,
                f32::from(event.position.x),
                event.modifiers.shift,
                context,
            ),
            None => return false,
        }
        true
    }

    pub(super) fn clip_drag_end(&mut self, context: &mut App) {
        let Some(drag) = self.clip.drag.take() else {
            return;
        };
        let Some(id) = self.clip.clip else {
            return;
        };
        let Some((_, clip)) = self.state(context).clip_with_track(id) else {
            return;
        };
        match drag {
            ClipDrag::Handle(_) => {
                let shown = self.state(context).shown_clip(&clip);
                self.state_mut(context).commit_resize_clip(shown);
            }
            // Clicked, it took the arrow keys; a double-click types it.
            ClipDrag::Field { moved, .. } => {
                if moved {
                    let shown = self.state(context).shown_clip(&clip);
                    self.state_mut(context).commit_resize_clip(shown);
                }
            }
            ClipDrag::Slider { slider, drag, .. } => self.end_slider(&clip, slider, drag, context),
            ClipDrag::Pan { .. } => {}
        }
    }
}

/// The ruler, the lane with the file's waveform on the grid (dimmed
/// outside what the clip plays, the loop tinted), the playhead where the
/// file plays at that moment, and the markers. The scrollbar is a widget
/// over its bottom.
fn paint_view(
    scene: &ClipScene,
    bounds: Rect,
    window: &mut Window,
    context: &mut gpui::App,
    waves: &mut crate::wave::WaveCache,
) {
    let theme = &scene.theme;
    let layout = scene.layout;
    let left = bounds.left;
    let (lane_top, lane_bottom) = (bounds.top + layout.lane.0, bounds.top + layout.lane.1);
    let lane = Rect::new(left, lane_top, bounds.width, lane_bottom - lane_top);
    let ruler = Rect::new(left, bounds.top, bounds.width, RULER_HEIGHT);
    let (file_left, file_right) = (left + scene.file.0, left + scene.file.1);
    let file = Rect::new(file_left, lane_top, file_right - file_left, lane.height);
    let column = |x0: f32, x1: f32, rect: Rect| Rect::new(x0, rect.top, x1 - x0, rect.height);

    fill(window, ruler, theme.bg_raised);
    let bars_top = bounds.top + LOOP_BAR_HEIGHT;
    for (x, text_shown, bar) in &scene.marks.labels {
        let x = (left + x).round();
        let top = if *bar { bars_top } else { bars_top + 6. };
        fill(
            window,
            Rect::new(
                x,
                top,
                1.,
                bounds.top + RULER_HEIGHT - markers::FLAG_SIZE.1 - top,
            ),
            theme.border,
        );
        let color = if *bar { theme.text } else { theme.text_muted };
        text(
            window,
            context,
            text_shown.clone(),
            (x + 3., bars_top),
            Align::LEFT_TOP,
            size::SMALL,
            color,
            Some(mono()),
        );
    }

    fill(window, lane, theme.bg_deep);
    for &(x, bar) in &scene.marks.lines {
        let color = if bar { theme.grid_bar } else { theme.grid_beat };
        fill(
            window,
            Rect::new((left + x).round(), lane_top, 1., lane.height),
            color,
        );
    }
    if let Some((loop_left, loop_right)) = scene.loop_x {
        fill(
            window,
            column(left + loop_left, left + loop_right, lane),
            with_alpha(scene.color, LOOP_TINT_ALPHA),
        );
    }
    if let Some(frames) = scene.clip.audio.total_frames() {
        paint_waveform(
            window,
            waves,
            file,
            file.intersect(lane),
            &scene.clip.audio.data,
            0.0..frames,
            scene.clip.audio.channels >= 2,
            hsla(scene.color),
            scene.clip.gain,
            scene.clip.pan,
        );
    }
    let dim = with_alpha(theme.bg_deep, DIM_ALPHA);
    let (played_left, played_right) = (left + scene.played.0, left + scene.played.1);
    for (from, to) in [(file_left, played_left), (played_right, file_right)] {
        if to > from {
            fill(window, column(from, to, lane), dim);
        }
    }
    if let Some(x) = scene.playhead {
        fill(
            window,
            Rect::new((left + x).round(), lane_top, 1., lane.height),
            theme.playhead,
        );
    }

    if let Some(area) = &scene.lit {
        let rect = area.rect(layout);
        fill_rounded(
            window,
            Rect::new(
                left + rect.left,
                bounds.top + rect.top,
                rect.width,
                rect.height,
            ),
            1.,
            theme.hover_overlay,
        );
    }
    markers::paint_markers(window, scene, bounds);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_is_laid_out_top_to_bottom() {
        let layout = Layout::of(200., 500.);
        assert_eq!(layout.lane, (RULER_HEIGHT, 192.));
    }
}
