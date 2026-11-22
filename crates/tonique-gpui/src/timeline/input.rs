//! The pointer on the timeline: pressing starts a drag (scrubbing, a loop,
//! a selection zone, moving or trimming clips) or places the edit cursor;
//! the root view follows the drag; a right-click opens a menu. The wheel
//! is [`super::scroll`]'s, moving clips [`super::moving`]'s.

use gpui::{
    Anchor, Context, Modifiers, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Window,
};
use tonique_engine::time::BeatPos;

use crate::{
    app::{PopupKind, Tonique},
    core::{clip::AudioClip, state::SelectionBounds},
    timeline::{
        LOOP_HEIGHT, RULER_HEIGHT,
        fades::{FadeDrag, FadePart},
        moving::ClipMove,
        rows::{ClipPart, rows},
        ruler::LoopGrab,
    },
};

/// Pixels the pointer moves before a press on a lane draws a zone.
const ZONE_THRESHOLD: f64 = 3.;

// One drag at a time, held in the view: a trim's clips make it large, but
// boxing them would buy nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Default)]
pub enum TimelineDrag {
    #[default]
    None,
    Scrub,
    Loop {
        grab: LoopGrab,
        moved: bool,
    },
    /// A press on a lane, off the clips' headers and edges: a click places
    /// the edit cursor, a drag draws a selection zone from `anchor` (a row
    /// and a beat).
    Zone {
        anchor: (usize, BeatPos),
        start: Point<Pixels>,
        moved: bool,
    },
    Move(ClipMove),
    Trim {
        original: AudioClip,
        start_edge: bool,
        current: Option<AudioClip>,
    },
    /// A fade's handle or curve, or a seam's.
    Fade(FadeDrag),
}

impl TimelineDrag {
    /// Whether it follows the pointer past the timeline's edges, scrolling.
    pub fn scrolls(&self) -> bool {
        match self {
            Self::Zone { moved, .. } => *moved,
            Self::Move(press) => press.dragged.is_some(),
            Self::Trim { .. } => true,
            _ => false,
        }
    }
}

impl Tonique {
    pub(super) fn on_timeline_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        window.focus(&self.focus, context);
        let (local_x, local_y) = self.local(event.position);
        let alt = event.modifiers.alt;
        self.timeline.drag = if self.on_playhead_head(local_x, local_y) {
            TimelineDrag::Scrub
        } else if local_y < LOOP_HEIGHT {
            let anchor = self.snapped(local_x, alt);
            TimelineDrag::Loop {
                grab: self
                    .loop_grab_at(local_x)
                    .unwrap_or(LoopGrab::Create { anchor }),
                moved: false,
            }
        } else if local_y < RULER_HEIGHT {
            self.state.seek(self.beats_at(local_x).max(BeatPos::ZERO));
            TimelineDrag::Scrub
        } else if self.in_footer(local_y) {
            // The master's footer only shows its output.
            TimelineDrag::None
        } else {
            self.press_lanes(event, local_x, local_y)
        };
        context.notify();
    }

    /// A press below the ruler: on a clip's edge, its header, or anywhere
    /// else (the edit cursor, or a zone).
    fn press_lanes(&mut self, event: &MouseDownEvent, local_x: f32, local_y: f32) -> TimelineDrag {
        let rows = rows(&self.state);
        if let Some(hit) = self.fade_at(&rows, local_x, local_y) {
            return self
                .press_fade(hit, local_y, event.click_count)
                .map_or(TimelineDrag::None, TimelineDrag::Fade);
        }
        let hit = self.clip_at(&rows, local_x, local_y);
        // A double-click on a clip shows its details in the bottom panel.
        if event.click_count >= 2
            && let Some((_, clip, _)) = &hit
        {
            self.state.select_clips(vec![clip.id]);
            self.show_clip_details(clip.id);
            return TimelineDrag::None;
        }
        match hit {
            Some((row, clip, part @ (ClipPart::TrimStart | ClipPart::TrimEnd))) => {
                self.state.select_track(&rows[row].track.id);
                TimelineDrag::Trim {
                    original: clip,
                    start_edge: matches!(part, ClipPart::TrimStart),
                    current: None,
                }
            }
            Some((row, clip, ClipPart::Header)) => {
                self.state.select_track(&rows[row].track.id);
                let shift = event.modifiers.shift;
                if shift {
                    self.state.toggle_clip_selected(clip.id);
                }
                TimelineDrag::Move(ClipMove {
                    id: clip.id,
                    track_index: rows[row].track.first_track_index,
                    start: event.position,
                    grab: self.beats_at(local_x),
                    shift,
                    dragged: None,
                })
            }
            hit => {
                // The body of a clip, or an empty lane.
                let row = hit
                    .map(|(row, ..)| row)
                    .or_else(|| self.row_at(&rows, local_y));
                if let Some(row) = row {
                    self.state.select_track(&rows[row].track.id);
                }
                // Zones span tracks: on a group's header, from its first.
                let last = self.state.track_count().saturating_sub(1);
                TimelineDrag::Zone {
                    anchor: (
                        row.map_or(last, |row| rows[row].track.first_track_index),
                        self.snapped(local_x, event.modifiers.alt),
                    ),
                    start: event.position,
                    moved: false,
                }
            }
        }
    }

    /// A right-click: a fade's menu over a fade, a clip's over a clip, the
    /// grid's elsewhere.
    pub(super) fn on_timeline_menu(
        &mut self,
        event: &MouseDownEvent,
        _: &mut Window,
        context: &mut Context<Self>,
    ) {
        let (local_x, local_y) = self.local(event.position);
        if self.in_footer(local_y) {
            return;
        }
        let rows = rows(&self.state);
        let kind = if let Some(hit) = self.fade_at(&rows, local_x, local_y) {
            let end = match hit.part {
                FadePart::Handle(end) | FadePart::Curve(end) => Some(end),
                FadePart::SeamHandle | FadePart::SeamCurve => None,
            };
            PopupKind::Fade(hit.clip.id, end)
        } else if let Some((_, clip, _)) = self.clip_at(&rows, local_x, local_y) {
            if !self.state.is_clip_selected(clip.id) {
                self.state.select_clips(vec![clip.id]);
            }
            PopupKind::Clip(clip.id)
        } else {
            PopupKind::Grid
        };
        self.toggle_popup(kind, event.position, Anchor::TopLeft);
        context.notify();
    }

    /// Follows the pointer during a timeline drag. Returns whether anything
    /// changed.
    pub fn timeline_drag_move(&mut self, event: &MouseMoveEvent) -> bool {
        if matches!(self.timeline.drag, TimelineDrag::None) {
            return false;
        }
        self.timeline.pointer = Some((event.position, event.modifiers));
        self.follow_timeline_drag(event.position, &event.modifiers);
        true
    }

    /// Follows the pointer at `position` (in the window) with the drag:
    /// when it moves, and when the timeline scrolls under it.
    pub(super) fn follow_timeline_drag(&mut self, position: Point<Pixels>, modifiers: &Modifiers) {
        let (local_x, local_y) = self.local(position);
        let alt = modifiers.alt;
        self.timeline.drag = match std::mem::take(&mut self.timeline.drag) {
            TimelineDrag::None => TimelineDrag::None,
            TimelineDrag::Scrub => {
                self.state.seek(self.beats_at(local_x).max(BeatPos::ZERO));
                TimelineDrag::Scrub
            }
            TimelineDrag::Loop { grab, .. } => {
                self.drag_loop(grab, local_x, alt);
                TimelineDrag::Loop { grab, moved: true }
            }
            TimelineDrag::Zone {
                anchor,
                start,
                moved,
            } => {
                let moved = moved || (position - start).magnitude() > ZONE_THRESHOLD;
                if moved {
                    self.draw_zone(anchor, local_x, local_y, alt);
                }
                TimelineDrag::Zone {
                    anchor,
                    start,
                    moved,
                }
            }
            TimelineDrag::Move(press) => self
                .follow_clip_move(press, position, modifiers)
                .map_or(TimelineDrag::None, TimelineDrag::Move),
            TimelineDrag::Trim {
                original,
                start_edge,
                ..
            } => {
                let clip = self.trimmed(&original, start_edge, local_x, modifiers);
                self.state.resize_clip(clip.clone());
                TimelineDrag::Trim {
                    original,
                    start_edge,
                    current: Some(clip),
                }
            }
            TimelineDrag::Fade(mut drag) => {
                self.follow_fade(&mut drag, local_x, local_y);
                TimelineDrag::Fade(drag)
            }
        };
    }

    /// Selects the clips in the zone from `anchor` (a track's index and a
    /// beat) to the pointer, on the tracks between, snapped as trims are.
    fn draw_zone(&mut self, anchor: (usize, BeatPos), local_x: f32, local_y: f32, alt: bool) {
        let track_count = self.state.track_count();
        if track_count == 0 {
            return;
        }
        let rows = rows(&self.state);
        let track = self.pointer_track(&rows, local_y).min(track_count - 1);
        let beats = self.snapped(local_x, alt);
        self.state
            .select_in_bounds(SelectionBounds::between(anchor, (track, beats)));
    }

    /// `original` with an edge at the pointer, snapped to the grid or
    /// other clips' edges, the loop and the edit cursor (unless Alt
    /// inverts snapping). With Shift, stretched there instead: the same
    /// audio, played faster or slower.
    fn trimmed(
        &self,
        original: &AudioClip,
        start_edge: bool,
        local_x: f32,
        modifiers: &Modifiers,
    ) -> AudioClip {
        let state = &self.state;
        let beats = self.beats_at(local_x);
        let at = if state.grid.snaps(modifiers.alt) {
            let targets = state.snap_targets(&[original.id]);
            state.grid.snap_to_targets(beats, &targets).0
        } else {
            beats
        };
        let (mut clip, bpm) = (original.clone(), state.bpm());
        match (start_edge, modifiers.shift) {
            (true, false) => clip.trim_start_at(at, bpm),
            (false, false) => clip.trim_end_at(at, bpm),
            (true, true) => clip.stretch_start_at(at, bpm),
            (false, true) => clip.stretch_end_at(at, bpm),
        }
        clip
    }

    pub fn timeline_drag_end(&mut self, event: &MouseUpEvent) {
        self.timeline.pointer = None;
        match std::mem::take(&mut self.timeline.drag) {
            // A click on the loop toggles looping; off it, it seeks.
            TimelineDrag::Loop {
                grab: LoopGrab::Create { .. },
                moved: false,
            } => {
                let (local_x, _) = self.local(event.position);
                self.state.seek(self.beats_at(local_x).max(BeatPos::ZERO));
            }
            TimelineDrag::Loop { moved: false, .. } => {
                let looping = self.state.looping();
                self.state.set_looping(!looping);
            }
            // A click on a lane places the edit cursor.
            TimelineDrag::Zone { moved: false, .. } => {
                let (local_x, _) = self.local(event.position);
                self.state.clear_clip_selection();
                self.state
                    .set_edit_cursor(self.snapped(local_x, event.modifiers.alt));
            }
            TimelineDrag::Move(ClipMove {
                dragged: Some(dragged),
                ..
            }) => {
                self.drop_clips(dragged);
                let over_lanes = self.timeline.bounds.get().contains(&event.position);
                let (_, local_y) = self.local(event.position);
                self.expand_on_hover(over_lanes.then_some(local_y), false);
            }
            // A click on a clip's header selects it alone.
            TimelineDrag::Move(ClipMove {
                id, shift: false, ..
            }) => self.state.select_clips(vec![id]),
            TimelineDrag::Trim {
                current: Some(clip),
                ..
            } => self.state.commit_resize_clip(clip),
            TimelineDrag::Fade(drag) => self.end_fade(drag),
            _ => {}
        }
    }

    /// Tracks the pointer over the timeline, for the cursor and lit
    /// headers. Returns whether that changed what's shown.
    pub fn timeline_hover(&mut self, event: &MouseMoveEvent) -> bool {
        let bounds = self.timeline.bounds.get();
        let hover = bounds
            .contains(&event.position)
            .then(|| self.local(event.position));
        let changed = hover.is_some() || self.timeline.hover.is_some();
        self.timeline.hover = hover;
        self.timeline.hover_alt = event.modifiers.alt;
        changed
    }
}
