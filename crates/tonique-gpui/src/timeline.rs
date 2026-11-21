//! The arrangement: the ruler (loop lane, bars, time), a lane per track with
//! its clips, the grid, the playhead and the edit cursor. Edits go through
//! Tonique's `ProjectState` and `GridService`, as in the egui app.
//!
//! Each frame, `timeline_view` takes what's shown out of the state into a
//! `Scene`; the canvas paints it. Pointer handling is on the wrapping div;
//! drags are then followed by the root view, so they work outside it.

use std::{cell::Cell, collections::HashSet, rc::Rc};

use gpui::{
    Bounds, ContentMask, Context, CursorStyle, ExternalPaths, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Point, ScrollDelta, ScrollEvent, Window,
    canvas, div, point, prelude::*, px,
};
use tonique_engine::{edit::ClipId, time::BeatPos};

use crate::{
    analysis::AudioInfo,
    app::Tonique,
    browser::DraggedAudio,
    cache::AUDIO_ANALYSIS_CACHE,
    core::{
        clip::AudioClip,
        grid::{GridLine, Viewport},
        state::ProjectState,
        track::{DEFAULT_TRACK_HEIGHT, TrackRow},
    },
    icons,
    theme::{
        BLACK, Color, Theme, WHITE, contrast, h, icon_font, mix, mono, over, size, with_alpha,
    },
    utils::format_time,
    wave::{R, WaveCache, paint_clip_waveform},
    widgets::paint_text,
};

pub const LOOP_H: f32 = 12.;
pub const BARS_H: f32 = 20.;
pub const TIME_H: f32 = 16.;
pub const RULER_H: f32 = LOOP_H + BARS_H + TIME_H;
/// Between rows, as `ROW_GAP` in Tonique's track headers.
pub const ROW_GAP: f32 = 3.;
const HEADER_H: f32 = 16.;
const CUT_GAP: f32 = 1.;
const TRIM_HANDLE: f32 = 7.;
const MIN_TRIM_HANDLE: f32 = 2.;
const BODY_TINT: f32 = 0.7;
const SELECTION_FILL: f32 = 0.8;
const COLLAPSED_WAVE: f32 = 0.4;
const SILENCED_BODY: f32 = 0.16;
const SILENCED_HEADER: f32 = 0.27;
const SILENCED_BORDER: f32 = 0.43;
const SILENCED_WAVE: f32 = 0.5;
const WAVE_CONTRAST: f32 = 3.;
const LANE_TINT: f32 = 0.06;

// One drag at a time, held in the view: a trim's clips make it large, but
// boxing them would buy nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Default)]
pub enum TlDrag {
    #[default]
    None,
    Scrub,
    Loop {
        anchor: BeatPos,
        moved: bool,
    },
    Move {
        id: ClipId,
        /// Beats between the clip's start and where it was grabbed.
        grab: f64,
        row: usize,
        start: Point<Pixels>,
        moved: bool,
        /// Rows and beats the selection is moved by.
        delta: (i32, f64),
    },
    Trim {
        original: AudioClip,
        start_edge: bool,
        current: Option<AudioClip>,
    },
}

/// Where audio dragged over the timeline would land.
#[derive(Clone)]
pub struct DropTarget {
    pub audio: AudioInfo,
    /// `None`: on a new track below the last.
    pub row: Option<usize>,
    pub position: BeatPos,
}

#[derive(Default)]
pub struct TimelineUi {
    pub bounds: Rc<Cell<Bounds<Pixels>>>,
    pub drag: TlDrag,
    /// Pointer over the timeline, local to it.
    pub hover: Option<(f32, f32)>,
    pub drop_target: Option<DropTarget>,
}

impl TimelineUi {
    pub fn is_dragging(&self) -> bool {
        !matches!(self.drag, TlDrag::None)
    }
}

/// The timeline's extent across the screen, for Tonique's grid.
#[derive(Clone, Copy)]
pub struct Span {
    pub left: f32,
    pub right: f32,
}

impl Viewport for Span {
    fn left(&self) -> f32 {
        self.left
    }
    fn right(&self) -> f32 {
        self.right
    }
}

/// A track's row: its top in the scrolled content, and height.
pub struct Row {
    pub track: TrackRow,
    pub top: f32,
}

pub fn rows(state: &ProjectState) -> Vec<Row> {
    let mut top = 0.;
    state
        .tracks()
        .map(|track| {
            let row = Row { top, track };
            top += row.track.height + ROW_GAP;
            row
        })
        .collect()
}

fn rows_height(rows: &[Row]) -> f32 {
    rows.last().map_or(0., |r| r.top + r.track.height + ROW_GAP)
}

/// What the pointer is over, in a clip.
enum ClipPart {
    TrimStart,
    TrimEnd,
    Header,
    Body,
}

impl Tonique {
    /// The timeline across the screen, in its own coordinates.
    fn viewport(&self) -> Span {
        Span {
            left: 0.,
            right: f32::from(self.timeline.bounds.get().size.width),
        }
    }

    fn height(&self) -> f32 {
        f32::from(self.timeline.bounds.get().size.height)
    }

    fn local(&self, p: Point<Pixels>) -> (f32, f32) {
        let o = self.timeline.bounds.get().origin;
        (f32::from(p.x - o.x), f32::from(p.y - o.y))
    }

    /// Screen y (local) of a row's top.
    fn row_y(&self, row: &Row) -> f32 {
        RULER_H + row.top - self.state.grid.offset.y
    }

    fn row_at(&self, rows: &[Row], y: f32) -> Option<usize> {
        rows.iter().position(|r| {
            let top = self.row_y(r);
            y >= top && y < top + r.track.height + ROW_GAP
        })
    }

    fn clip_rect(&self, clip: &AudioClip, row: &Row) -> R {
        let vp = self.viewport();
        let x = self.state.grid.beats_to_x(clip.position, vp);
        let x2 = self.state.grid.beats_to_x(clip.end(self.state.bpm()), vp);
        R::new(x, self.row_y(row), x2 - x, row.track.height)
    }

    fn clip_at(&self, rows: &[Row], x: f32, y: f32) -> Option<(usize, AudioClip, ClipPart)> {
        let i = self.row_at(rows, y)?;
        let row = &rows[i];
        row.track.clips.iter().rev().find_map(|clip| {
            let clip = self.state.shown_clip(clip);
            let r = self.clip_rect(&clip, row);
            if !r.contains(x, y) {
                return None;
            }
            let handle = (r.w / 4.).clamp(MIN_TRIM_HANDLE, TRIM_HANDLE);
            let part = if x < r.x + handle {
                ClipPart::TrimStart
            } else if x >= r.right() - handle {
                ClipPart::TrimEnd
            } else if y < r.y + HEADER_H {
                ClipPart::Header
            } else {
                ClipPart::Body
            };
            Some((i, clip, part))
        })
    }

    fn beats_at(&self, x: f32) -> BeatPos {
        self.state.grid.x_to_beats(x, self.viewport())
    }

    fn snapped(&self, x: f32, alt: bool) -> BeatPos {
        self.state
            .grid
            .snap(self.beats_at(x), alt)
            .max(BeatPos::ZERO)
    }

    fn on_timeline_down(
        &mut self,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let (x, y) = self.local(ev.position);
        let alt = ev.modifiers.alt;
        if y < LOOP_H {
            self.timeline.drag = TlDrag::Loop {
                anchor: self.snapped(x, alt),
                moved: false,
            };
        } else if y < RULER_H {
            self.timeline.drag = TlDrag::Scrub;
            self.state.seek(self.beats_at(x).max(BeatPos::ZERO));
        } else {
            let rows = rows(&self.state);
            match self.clip_at(&rows, x, y) {
                Some((row, clip, part @ (ClipPart::TrimStart | ClipPart::TrimEnd))) => {
                    self.state.select_track(&rows[row].track.id);
                    self.timeline.drag = TlDrag::Trim {
                        original: clip,
                        start_edge: matches!(part, ClipPart::TrimStart),
                        current: None,
                    };
                }
                Some((row, clip, ClipPart::Header)) => {
                    self.state.select_track(&rows[row].track.id);
                    if ev.modifiers.shift {
                        self.state.toggle_clip_selected(clip.id);
                    } else if !self.state.is_clip_selected(clip.id) {
                        self.state.select_clips(vec![clip.id]);
                    }
                    self.timeline.drag = TlDrag::Move {
                        id: clip.id,
                        grab: self.beats_at(x).0 - clip.position.0,
                        row,
                        start: ev.position,
                        moved: false,
                        delta: (0, 0.),
                    };
                }
                hit => {
                    // The body of a clip, or an empty lane: the edit cursor.
                    let row = hit.map(|(row, ..)| row).or_else(|| self.row_at(&rows, y));
                    if let Some(row) = row {
                        self.state.select_track(&rows[row].track.id);
                    }
                    self.state.clear_clip_selection();
                    self.state.set_edit_cursor(self.snapped(x, alt));
                }
            }
        }
        cx.notify();
    }

    /// Follows the pointer during a timeline drag. Returns whether anything
    /// changed.
    pub fn timeline_drag_move(&mut self, ev: &MouseMoveEvent) -> bool {
        let (x, y) = self.local(ev.position);
        let alt = ev.modifiers.alt;
        match std::mem::take(&mut self.timeline.drag) {
            TlDrag::None => return false,
            TlDrag::Scrub => {
                self.state.seek(self.beats_at(x).max(BeatPos::ZERO));
                self.timeline.drag = TlDrag::Scrub;
            }
            TlDrag::Loop { anchor, .. } => {
                let to = self.snapped(x, alt);
                if to != anchor {
                    let (a, b) = if to < anchor {
                        (to, anchor)
                    } else {
                        (anchor, to)
                    };
                    self.state.set_loop_range(a, b);
                    self.state.set_looping(true);
                }
                self.timeline.drag = TlDrag::Loop {
                    anchor,
                    moved: true,
                };
            }
            TlDrag::Move {
                id,
                grab,
                row,
                start,
                moved,
                ..
            } => {
                let moved = moved || (ev.position - start).magnitude() > 3.;
                let rows = rows(&self.state);
                let delta = match self.state.clip_with_track(id) {
                    Some((_, clip)) if moved => {
                        let to = self
                            .state
                            .grid
                            .snap(BeatPos(self.beats_at(x).0 - grab), alt)
                            .max(BeatPos::ZERO);
                        let to_row = self.row_at(&rows, y).unwrap_or(if y < RULER_H {
                            0
                        } else {
                            rows.len().saturating_sub(1)
                        });
                        (to_row as i32 - row as i32, to.0 - clip.position.0)
                    }
                    _ => (0, 0.),
                };
                self.timeline.drag = TlDrag::Move {
                    id,
                    grab,
                    row,
                    start,
                    moved,
                    delta,
                };
            }
            TlDrag::Trim {
                original,
                start_edge,
                ..
            } => {
                let bpm = self.state.bpm();
                let mut clip = original.clone();
                let at = self.snapped(x, alt);
                if start_edge {
                    clip.trim_start_at(at, bpm);
                } else {
                    clip.trim_end_at(at, bpm);
                }
                self.state.resize_clip(clip.clone());
                self.timeline.drag = TlDrag::Trim {
                    original,
                    start_edge,
                    current: Some(clip),
                };
            }
        }
        true
    }

    pub fn timeline_drag_end(&mut self, _: &MouseUpEvent) {
        match std::mem::take(&mut self.timeline.drag) {
            TlDrag::Loop { moved: false, .. } => {
                let looping = self.state.looping();
                self.state.set_looping(!looping);
            }
            TlDrag::Move {
                moved: true,
                delta: (drow, dbeats),
                ..
            } if drow != 0 || dbeats != 0. => {
                let rows = rows(&self.state);
                let ids: Vec<ClipId> = self.state.selected_clips().to_vec();
                self.state.begin_batch();
                for id in &ids {
                    let Some((track, clip)) = self.state.clip_with_track(*id) else {
                        continue;
                    };
                    let Some(from) = rows.iter().position(|r| r.track.id == track.id) else {
                        continue;
                    };
                    let to = (from as i32 + drow).clamp(0, rows.len() as i32 - 1) as usize;
                    let position = BeatPos(clip.position.0 + dbeats);
                    self.state.move_clip(id, &rows[to].track.id, position, &ids);
                }
                self.state.commit_batch();
            }
            TlDrag::Trim {
                current: Some(clip),
                ..
            } => self.state.commit_resize_clip(clip),
            _ => {}
        }
    }

    /// Tracks the pointer over the timeline, for the cursor and lit
    /// headers. Returns whether that changed what's shown.
    pub fn timeline_hover(&mut self, ev: &MouseMoveEvent) -> bool {
        let bounds = self.timeline.bounds.get();
        let hover = bounds
            .contains(&ev.position)
            .then(|| self.local(ev.position));
        let changed = hover.is_some() || self.timeline.hover.is_some();
        self.timeline.hover = hover;
        changed
    }

    fn on_timeline_scroll(&mut self, ev: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (x, _) = self.local(ev.position);
        let (dx, dy) = match ev.delta {
            ScrollDelta::Lines(p) => (p.x * 40., p.y * 40.),
            ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
        };
        let vp = self.viewport();
        if ev.modifiers.control || ev.modifiers.platform {
            self.state.grid.zoom_around(-dy, x, vp);
        } else if ev.modifiers.shift || dx.abs() > dy.abs() {
            let d = if dx.abs() > dy.abs() { dx } else { dy };
            self.state.grid.offset.x = (self.state.grid.offset.x - d).max(0.);
        } else {
            let rows = rows(&self.state);
            let max =
                (rows_height(&rows) + DEFAULT_TRACK_HEIGHT - (self.height() - RULER_H)).max(0.);
            self.state.grid.offset.y = (self.state.grid.offset.y - dy).clamp(0., max);
        }
        cx.notify();
    }

    // Dropping audio, from the browser or the system.

    fn drop_target_at(
        &self,
        position: Point<Pixels>,
        audio: AudioInfo,
        alt: bool,
    ) -> Option<DropTarget> {
        if !self.timeline.bounds.get().contains(&position) {
            return None;
        }
        audio.duration?;
        audio.load_peaks();
        let (x, y) = self.local(position);
        let rows = rows(&self.state);
        Some(DropTarget {
            audio,
            row: self.row_at(&rows, y.max(RULER_H)),
            position: self.snapped(x, alt),
        })
    }

    /// Places `paths` from `target` on: the first on its track (or a new
    /// one), the others on the tracks below, adding tracks as needed.
    fn drop_audio(&mut self, paths: &[std::path::PathBuf], target: Option<DropTarget>) {
        let Some(target) = target else { return };
        let infos: Vec<AudioInfo> = paths
            .iter()
            .filter_map(|p| AUDIO_ANALYSIS_CACHE.get_or_analyze(p.clone()))
            .filter(|a| a.duration.is_some())
            .collect();
        if infos.is_empty() {
            return;
        }
        let rows = rows(&self.state);
        self.state.begin_batch();
        for (i, audio) in infos.into_iter().enumerate() {
            let track = match target.row.map(|r| r + i).and_then(|r| rows.get(r)) {
                Some(row) => row.track.id,
                None => self.state.add_track(),
            };
            let clip = AudioClip::new(
                self.state.new_clip_id(),
                audio,
                target.position,
                self.state.bpm(),
            );
            self.state.add_clips(&track, vec![clip]);
        }
        self.state.commit_batch();
        self.timeline.drop_target = None;
    }

    fn cursor(&self) -> CursorStyle {
        match &self.timeline.drag {
            TlDrag::Move { .. } => return CursorStyle::ClosedHand,
            TlDrag::Trim { .. } | TlDrag::Scrub => return CursorStyle::ResizeLeftRight,
            _ => {}
        }
        let Some((x, y)) = self.timeline.hover else {
            return CursorStyle::Arrow;
        };
        if y < RULER_H {
            return CursorStyle::PointingHand;
        }
        match self.clip_at(&rows(&self.state), x, y) {
            Some((_, _, ClipPart::TrimStart | ClipPart::TrimEnd)) => CursorStyle::ResizeLeftRight,
            Some((_, _, ClipPart::Header)) => CursorStyle::OpenHand,
            _ => CursorStyle::IBeam,
        }
    }

    pub fn timeline_view(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let scene = self.scene();
        let waves = self.waves.clone();
        let slot = self.timeline.bounds.clone();

        div()
            .id("timeline")
            .flex_1()
            .min_w_0()
            .h_full()
            .cursor(self.cursor())
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_timeline_down))
            .on_scroll(cx.listener(Self::on_timeline_scroll))
            .on_drag_move(
                cx.listener(|this, ev: &gpui::DragMoveEvent<DraggedAudio>, _, cx| {
                    let audio = AUDIO_ANALYSIS_CACHE.get_or_analyze(ev.drag(cx).path.clone());
                    this.timeline.drop_target = audio.and_then(|a| {
                        this.drop_target_at(ev.event.position, a, ev.event.modifiers.alt)
                    });
                    cx.notify();
                }),
            )
            .on_drag_move(
                cx.listener(|this, ev: &gpui::DragMoveEvent<ExternalPaths>, _, cx| {
                    let first = ev.drag(cx).paths().first().cloned();
                    let audio = first.and_then(|p| AUDIO_ANALYSIS_CACHE.get_or_analyze(p));
                    this.timeline.drop_target = audio.and_then(|a| {
                        this.drop_target_at(ev.event.position, a, ev.event.modifiers.alt)
                    });
                    cx.notify();
                }),
            )
            .on_drop(cx.listener(|this, dragged: &DraggedAudio, _, cx| {
                let target = this.timeline.drop_target.take();
                this.drop_audio(std::slice::from_ref(&dragged.path), target);
                cx.notify();
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                // Without a move during the drag (some systems), from where
                // the pointer is now.
                let target = this.timeline.drop_target.take().or_else(|| {
                    let audio =
                        AUDIO_ANALYSIS_CACHE.get_or_analyze(paths.paths().first()?.clone())?;
                    this.drop_target_at(window.mouse_position(), audio, false)
                });
                this.drop_audio(paths.paths(), target);
                cx.notify();
            }))
            .child(
                canvas(
                    move |bounds, _, _| slot.set(bounds),
                    move |bounds, _, window, cx| {
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            crate::profile::time("paint.timeline", || {
                                paint(&scene, bounds, window, cx, &mut waves.borrow_mut())
                            })
                        })
                    },
                )
                .size_full(),
            )
    }

    /// What's shown, out of the state.
    fn scene(&self) -> Scene {
        let state = &self.state;
        let grid = &state.grid;
        let t = &self.theme;
        let vp = self.viewport();
        // Lines and labels a little past the edge, in case the view grew.
        let wide = Span {
            right: vp.right + 400.,
            ..vp
        };
        let (from, to) = grid.visible_beats(wide);
        let x = |b: BeatPos| grid.beats_to_x(b, vp);
        let lines = grid
            .lines(from, to, grid.line_step())
            .into_iter()
            .map(|(b, line)| (x(b), matches!(line, GridLine::Bar(_))))
            .collect();
        let labels = grid
            .lines(from, to, grid.label_step(from, to))
            .into_iter()
            .map(|(b, line)| {
                let text = match line {
                    GridLine::Bar(n) => format!("{n}"),
                    GridLine::Beat { bar, beat } => format!("{bar}.{beat}"),
                    GridLine::Division {
                        bar,
                        beat,
                        sub: Some(s),
                    } => format!("{bar}.{beat}.{s}"),
                    GridLine::Division {
                        bar,
                        beat,
                        sub: None,
                    } => format!("{bar}.{beat}"),
                };
                (x(b), text, matches!(line, GridLine::Bar(_)))
            })
            .collect();
        let bpm = state.bpm();
        let time_step = grid.time_step(bpm);
        let secs = |b: BeatPos| b.0 * 60. / bpm as f64;
        let first = (secs(from).max(0.) / time_step).floor() as i64;
        let last = (secs(to) / time_step).ceil() as i64;
        let times = (first..=last)
            .map(|i| {
                let s = i as f64 * time_step;
                let mut text = format_time(s);
                if time_step >= 1. {
                    text.truncate(text.len() - 4);
                }
                (x(BeatPos(s * bpm as f64 / 60.)), text)
            })
            .collect();

        let rows = rows(state);
        let (moving, delta) = match &self.timeline.drag {
            TlDrag::Move {
                moved: true, delta, ..
            } => (
                state
                    .selected_clips()
                    .iter()
                    .copied()
                    .collect::<HashSet<_>>(),
                *delta,
            ),
            _ => (HashSet::new(), (0, 0.)),
        };
        let hovered_header = match (&self.timeline.drag, self.timeline.hover) {
            (TlDrag::None, Some((hx, hy))) => match self.clip_at(&rows, hx, hy) {
                Some((_, clip, ClipPart::Header)) => Some(clip.id),
                _ => None,
            },
            _ => None,
        };
        let mut scene_rows = Vec::new();
        let mut clips = Vec::new();
        let mut moved = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let y = self.row_y(row);
            let tr = &row.track;
            scene_rows.push(SceneRow {
                y,
                h: tr.height,
                color: tr.color,
                selected: tr.selected,
            });
            for clip in &tr.clips {
                let clip = state.shown_clip(clip);
                let r = self.clip_rect(&clip, row);
                let selected = state.is_clip_selected(clip.id);
                let item = SceneClip {
                    lit: hovered_header == Some(clip.id),
                    rect: r,
                    color: tr.color,
                    silenced: tr.is_silenced(),
                    collapsed: tr.collapsed,
                    selected,
                    clip,
                };
                if moving.contains(&item.clip.id) {
                    // Drawn where it would go, over everything.
                    let to = (i as i32 + delta.0).clamp(0, rows.len() as i32 - 1) as usize;
                    let target = &rows[to];
                    let mut item = item;
                    item.rect.x += grid.beats_to_width(delta.1);
                    item.rect.y = self.row_y(target);
                    item.rect.h = target.track.height;
                    item.color = target.track.color;
                    moved.push(item);
                } else {
                    clips.push(item);
                }
            }
        }
        clips.extend(moved);

        let drop = self.timeline.drop_target.as_ref().map(|d| {
            let (y, h, color) = match d.row.and_then(|r| rows.get(r)) {
                Some(row) => (self.row_y(row), row.track.height, row.track.color),
                None => (
                    RULER_H + rows_height(&rows) - grid.offset.y,
                    DEFAULT_TRACK_HEIGHT,
                    state
                        .new_track_colors(1)
                        .first()
                        .copied()
                        .unwrap_or(t.text_muted),
                ),
            };
            let clip = AudioClip::new(ClipId(0), d.audio.clone(), d.position, bpm);
            let rect = R::new(x(d.position), y, grid.beats_to_width(clip.length), h);
            (
                SceneClip {
                    clip,
                    rect,
                    color,
                    selected: false,
                    lit: false,
                    silenced: false,
                    collapsed: false,
                },
                d.row.is_none(),
            )
        });

        let (loop_a, loop_b) = state.loop_range();
        Scene {
            theme: t.clone(),
            ppb: grid.pixels_per_beat(),
            bpm,
            rows: scene_rows,
            clips,
            drop,
            lines,
            labels,
            times,
            playhead: x(state.playhead()),
            edit_cursor: x(state.edit_cursor()),
            looping: state.looping(),
            loop_x: (x(loop_a), x(loop_b)),
        }
    }
}

struct SceneRow {
    y: f32,
    h: f32,
    color: Color,
    selected: bool,
}

struct SceneClip {
    clip: AudioClip,
    /// Local to the timeline.
    rect: R,
    color: Color,
    selected: bool,
    lit: bool,
    silenced: bool,
    collapsed: bool,
}

struct Scene {
    theme: Theme,
    ppb: f32,
    bpm: f32,
    rows: Vec<SceneRow>,
    clips: Vec<SceneClip>,
    /// The clip a drop would add, and whether on a new track.
    drop: Option<(SceneClip, bool)>,
    /// x, and whether it's a bar line.
    lines: Vec<(f32, bool)>,
    labels: Vec<(f32, String, bool)>,
    times: Vec<(f32, String)>,
    playhead: f32,
    edit_cursor: f32,
    looping: bool,
    loop_x: (f32, f32),
}

fn paint(
    s: &Scene,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut gpui::App,
    waves: &mut WaveCache,
) {
    let t = &s.theme;
    let (ox, oy) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let (w, hgt) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let r = |x: f32, y: f32, rw: f32, rh: f32| R::new(ox + x, oy + y, rw, rh);
    let fill =
        |window: &mut Window, rr: R, c: Color| window.paint_quad(gpui::fill(rr.bounds(), h(c)));

    // Lanes, then the grid over them.
    fill(window, r(0., 0., w, hgt), t.bg_base);
    let lanes = R::new(ox, oy + RULER_H, w, hgt - RULER_H);
    for row in &s.rows {
        let lane = r(0., row.y, w, row.h).intersect(lanes);
        if row.selected && !lane.is_empty() {
            fill(window, lane, mix(t.bg_base, row.color, LANE_TINT));
        }
        let gap = r(0., row.y + row.h, w, ROW_GAP).intersect(lanes);
        if !gap.is_empty() {
            fill(window, gap, t.separator);
        }
    }
    if s.looping {
        let (a, b) = s.loop_x;
        fill(
            window,
            r(a, RULER_H, b - a, hgt - RULER_H).intersect(lanes),
            t.loop_region,
        );
    }
    for &(x, bar) in &s.lines {
        fill(
            window,
            r(x.round(), RULER_H, 1., hgt - RULER_H),
            if bar { t.grid_bar } else { t.grid_beat },
        );
    }

    // Clips, clipped to the lanes.
    window.with_content_mask(
        Some(ContentMask {
            bounds: lanes.bounds(),
        }),
        |window| {
            for c in &s.clips {
                paint_clip(
                    window,
                    cx,
                    waves,
                    t,
                    c,
                    lanes,
                    (ox, oy),
                    s.ppb,
                    s.bpm,
                    false,
                );
            }
            if let Some((c, new_track)) = &s.drop {
                if *new_track {
                    fill(
                        window,
                        r(0., c.rect.y, w, c.rect.h).intersect(lanes),
                        with_alpha(c.color, 24),
                    );
                }
                paint_clip(window, cx, waves, t, c, lanes, (ox, oy), s.ppb, s.bpm, true);
            }
        },
    );

    // The ruler: loop lane, bars, time.
    fill(window, r(0., 0., w, RULER_H), t.bg_panel);
    fill(window, r(0., RULER_H - 1., w, 1.), t.separator);
    let (a, b) = s.loop_x;
    let bar_color = if s.looping { t.accent } else { t.text_disabled };
    window.paint_quad(
        gpui::fill(
            r(a, 3., (b - a).max(2.), LOOP_H - 5.).bounds(),
            h(bar_color),
        )
        .corner_radii(px(3.)),
    );
    for (x, text, bar) in &s.labels {
        fill(
            window,
            r(
                x.round(),
                LOOP_H + (if *bar { 2. } else { 9. }),
                1.,
                BARS_H - (if *bar { 2. } else { 9. }),
            ),
            t.border,
        );
        let color = if *bar { t.text } else { t.text_muted };
        paint_text(
            window,
            cx,
            text.clone(),
            ox + x + 4.,
            oy + LOOP_H + 3.,
            size::LABEL,
            h(color),
            mono(),
        );
    }
    for (x, text) in &s.times {
        fill(
            window,
            r(x.round(), LOOP_H + BARS_H + 8., 1., TIME_H - 8.),
            t.separator,
        );
        paint_text(
            window,
            cx,
            text.clone(),
            ox + x + 4.,
            oy + LOOP_H + BARS_H + 2.,
            size::SMALL,
            h(t.text_disabled),
            mono(),
        );
    }

    // The edit cursor, then the playhead over it.
    if (s.edit_cursor - s.playhead).abs() >= 1. {
        fill(
            window,
            r(s.edit_cursor.round(), RULER_H, 1., hgt - RULER_H),
            with_alpha(t.edit_cursor, 120),
        );
    }
    let x = s.playhead.round();
    if (0.0..=w).contains(&x) {
        fill(window, r(x, LOOP_H, 1., hgt - LOOP_H), t.playhead);
        let mut head = PathBuilder::fill();
        head.move_to(point(px(ox + x - 5.), px(oy + LOOP_H)));
        head.line_to(point(px(ox + x + 6.), px(oy + LOOP_H)));
        head.line_to(point(px(ox + x + 0.5), px(oy + LOOP_H + 7.)));
        head.close();
        if let Ok(path) = head.build() {
            window.paint_path(path, h(t.playhead));
        }
    }
}

/// A clip as `ClipView::paint` draws it: its body a tint of its track's
/// colour with its waveform, its header solid, its name on it.
#[allow(clippy::too_many_arguments)]
fn paint_clip(
    window: &mut Window,
    cx: &mut gpui::App,
    waves: &mut WaveCache,
    t: &Theme,
    c: &SceneClip,
    lanes: R,
    (ox, oy): (f32, f32),
    ppb: f32,
    bpm: f32,
    ghost: bool,
) {
    let mut rect = R::new(ox + c.rect.x, oy + c.rect.y, c.rect.w, c.rect.h);
    if rect.w > 2. * CUT_GAP {
        rect.w -= CUT_GAP;
    }
    if rect.right() < lanes.x || rect.x > lanes.right() {
        return;
    }
    let color = c.color;
    let tint = |amount: f32| mix(t.bg_base, color, amount);
    let missing = c.clip.audio.missing;
    let (body, header, ink, outline) = if c.silenced || missing {
        (
            tint(SILENCED_BODY),
            tint(SILENCED_HEADER),
            t.text_muted,
            tint(SILENCED_BORDER),
        )
    } else {
        (tint(BODY_TINT), color, t.text_on(color), color)
    };
    let body = if c.selected {
        mix(color, t.clip_selected, SELECTION_FILL)
    } else {
        body
    };
    let alpha = |c: Color| if ghost { with_alpha(c, 170) } else { c };

    window.paint_quad(gpui::quad(
        rect.bounds(),
        px(2.),
        h(alpha(body)),
        px(1.),
        h(outline),
        gpui::BorderStyle::Solid,
    ));
    // The waveform: black, or white on what's too dark for it.
    let wave = R::new(rect.x, rect.y + HEADER_H, rect.w, rect.h - HEADER_H);
    let mut wave_ink = if c.silenced {
        tint(SILENCED_WAVE)
    } else if contrast(BLACK, body) >= WAVE_CONTRAST {
        BLACK
    } else {
        WHITE
    };
    if c.collapsed {
        wave_ink = mix(body, wave_ink, COLLAPSED_WAVE);
    }
    if wave.h > 2. {
        let clip_rect = R::new(ox + c.rect.x, wave.y, c.rect.w, wave.h);
        paint_clip_waveform(
            window,
            waves,
            &c.clip,
            clip_rect,
            wave.intersect(lanes),
            ppb,
            bpm,
            h(alpha(wave_ink)),
        );
    }
    let header = if c.lit {
        over(header, t.hover_overlay)
    } else {
        header
    };
    window.paint_quad(
        gpui::fill(
            R::new(rect.x, rect.y, rect.w, HEADER_H.min(rect.h)).bounds(),
            h(alpha(header)),
        )
        .corner_radii(gpui::Corners {
            top_left: px(2.),
            top_right: px(2.),
            bottom_left: px(0.),
            bottom_right: px(0.),
        }),
    );
    // The name, kept in view when the clip starts off screen.
    let name_x = rect.x.max(lanes.x) + 4.;
    if rect.right() - name_x > 12. {
        let mask = R::new(rect.x, rect.y, rect.w - 2., HEADER_H).bounds();
        window.with_content_mask(Some(ContentMask { bounds: mask }), |window| {
            // A missing file: a warning before its name, as in Tonique.
            let mut x = name_x;
            if missing {
                let warning = icons::WARNING;
                x += paint_text(
                    window,
                    cx,
                    warning,
                    x,
                    rect.y + 2.,
                    size::SMALL,
                    h(t.warning),
                    icon_font(false),
                ) + 3.;
            }
            paint_text(
                window,
                cx,
                c.clip.audio.name.clone(),
                x,
                rect.y + 2.,
                size::SMALL,
                h(ink),
                mono(),
            );
        });
    }
    if !c.clip.audio.data.is_ready() && !missing {
        window.paint_quad(gpui::fill(wave.intersect(lanes).bounds(), h(t.shadow)));
    }
}
