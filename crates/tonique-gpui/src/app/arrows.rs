//! The value control holding the arrow keys, as the egui app's focused
//! one: clicked, a control holds them and each press steps it (a big step
//! with Shift), as one undo step; Esc or a press anywhere else lets go.
//! The other shortcuts still run meanwhile.

use gpui::{App, Context, Keystroke, Window};
use tonique_engine::edit::TrackId;

use crate::{
    app::Tonique,
    bottom::{ClipArrows, KnobArrows},
    headers::Param,
    widgets::{NumberField, arrow_press},
};

/// A control holding the arrow keys.
#[derive(Clone)]
pub enum ArrowHolder {
    /// A track header's volume or pan.
    Header(TrackId, Param),
    /// A value of the clip inspector's.
    Clip(ClipArrows),
    /// An effect's knob.
    Knob(KnobArrows),
    /// The metronome's level, in settings.
    MetronomeLevel,
    /// A number field, found by `get` and set by `set`, known by `id`.
    Number {
        id: &'static str,
        get: fn(&mut Tonique) -> &mut NumberField,
        set: fn(&mut Tonique, f32, &mut App),
    },
}

impl ArrowHolder {
    /// Whether they're the same control.
    pub fn is(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Header(track, param), Self::Header(other_track, other_param)) => {
                track == other_track && param == other_param
            }
            (Self::Clip(arrows), Self::Clip(other)) => arrows == other,
            (Self::Knob(arrows), Self::Knob(other)) => arrows.is(other),
            (Self::MetronomeLevel, Self::MetronomeLevel) => true,
            (Self::Number { id, .. }, Self::Number { id: other, .. }) => id == other,
            _ => false,
        }
    }
}

impl Tonique {
    /// `holder` takes the arrow keys, from whichever had them.
    pub fn hold_arrows(&mut self, holder: ArrowHolder) {
        self.arrows = Some(holder);
    }

    /// Whether `holder` has the arrow keys: outlined in the accent.
    pub fn holds_arrows(&self, holder: &ArrowHolder) -> bool {
        self.arrows.as_ref().is_some_and(|held| held.is(holder))
    }

    /// Whether the number field known by `id` has the arrow keys.
    pub fn holds_number(&self, id: &str) -> bool {
        matches!(&self.arrows, Some(ArrowHolder::Number { id: held, .. }) if *held == id)
    }

    /// No control has the arrow keys any more.
    pub fn release_arrows(&mut self) {
        self.arrows = None;
    }

    /// Steps the control holding the arrow keys by `keystroke`, when it's
    /// an arrow and the keyboard is the app's. Returns whether it did.
    pub(super) fn arrow_key(
        &mut self,
        keystroke: &Keystroke,
        window: &Window,
        context: &mut Context<Self>,
    ) -> bool {
        let (Some(presses), Some(holder)) = (arrow_press(keystroke), self.arrows.clone()) else {
            return false;
        };
        if !self.has_keyboard(window, context) {
            return false;
        }
        match holder {
            ArrowHolder::Header(track, param) => self.step_header(track, param, presses, context),
            ArrowHolder::Clip(arrows) => self.step_clip(arrows, presses, context),
            ArrowHolder::Knob(arrows) => self.step_knob(&arrows, presses, context),
            ArrowHolder::MetronomeLevel => self.step_metronome_level(presses, context),
            ArrowHolder::Number { get, set, .. } => {
                if let Some(value) = get(self).stepped(presses) {
                    set(self, value, context);
                }
            }
        }
        true
    }
}
