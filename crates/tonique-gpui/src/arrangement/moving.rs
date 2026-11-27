//! Clips dragged by their header: the core's [`ClipMove`] decides where
//! they land and what landing does; this follows the pointer with it once
//! it moved far enough. With Ctrl (or Super), copies of the selection move.

use crate::arrangement::Arrangement;
use crate::model::HoldsProject;
use gpui::{App, Modifiers, Pixels, Point};
use tonique_engine::{edit::ClipId, time::BeatPos};

use tonique_core::project::clip_drag::ClipMove;

/// Pixels the pointer moves before a press on a clip becomes a move.
const MOVE_THRESHOLD: f64 = 3.;

/// A press on a clip's header, until it's released.
pub struct ClipPress {
    /// The clip grabbed.
    pub id: ClipId,
    /// Where it was pressed, in the window, and the beat there.
    pub start: Point<Pixels>,
    pub grab: BeatPos,
    /// Shift was held: the press toggled the clip in the selection, so a
    /// click doesn't select it alone.
    pub shift: bool,
    /// Once the pointer moved far enough.
    pub moving: Option<ClipMove>,
}

fn duplicating(modifiers: &Modifiers) -> bool {
    modifiers.control || modifiers.platform
}

impl Arrangement {
    /// Follows the pointer with a press on a clip's header. Returns `None`
    /// when a copying drag lost its Ctrl: it's dropped without copying.
    pub(super) fn follow_clip_move(
        &mut self,
        mut press: ClipPress,
        position: Point<Pixels>,
        modifiers: &Modifiers,
        context: &mut App,
    ) -> Option<ClipPress> {
        if press.moving.is_none() {
            if (position - press.start).magnitude() <= MOVE_THRESHOLD {
                return Some(press);
            }
            let duplicate = duplicating(modifiers);
            press.moving = Some(ClipMove::start(
                &mut self.state_mut(context),
                press.id,
                press.grab,
                duplicate,
            ));
        }
        let mut moving = press.moving.take()?;
        if moving.duplicate && !duplicating(modifiers) {
            self.expand_on_hover(None, false, context);
            return None;
        }
        let (local_x, local_y) = self.local(position);
        let over_lanes = self.timeline.bounds.get().contains(&position);
        self.expand_on_hover(over_lanes.then_some(local_y), true, context);
        let pointer = self.beats_at(local_x, context);
        let pointer_track = self.pointer_track(&self.rows(context), local_y, context);
        let state = self.state(context);
        moving.place(
            state,
            self.timeline_view(context),
            pointer,
            pointer_track,
            modifiers.alt,
        );
        press.moving = Some(moving);
        Some(press)
    }

    /// Lands the moved clips, as one undo step.
    pub(super) fn land_clips(&mut self, moving: ClipMove, context: &mut App) {
        moving.land(&mut self.state_mut(context));
    }
}
