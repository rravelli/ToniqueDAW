//! The value control holding the arrow keys, as the egui app's focused
//! one: clicked, a control holds them and each press steps it (a big step
//! with Shift), as one undo step; Esc or a press anywhere else lets go.
//! The other shortcuts still run meanwhile.

use std::rc::Rc;

use gpui::{App, Context, Global, Keystroke, Window};
use tonique_engine::edit::TrackId;

use crate::{
    app::Tonique,
    bottom::{ClipArrows, KnobArrows},
    headers::Param,
    utils::steps::Presses,
    widgets::arrow_press,
};

/// Steps a control of a view's by some arrow presses.
pub type Stepper = Rc<dyn Fn(Presses, &mut App)>;

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
    /// A number field known by `id`, in whichever view, stepped by `step`.
    Number { id: &'static str, step: Stepper },
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

/// The control holding the arrow keys, if any, app-wide: any view's
/// control can take them.
#[derive(Default)]
struct HeldArrows(Option<ArrowHolder>);

impl Global for HeldArrows {}

/// `holder` takes the arrow keys, from whichever had them.
pub fn hold(holder: ArrowHolder, context: &mut App) {
    context.set_global(HeldArrows(Some(holder)));
}

/// Whether `holder` has the arrow keys: outlined in the accent.
pub fn holds(holder: &ArrowHolder, context: &App) -> bool {
    held(context).is_some_and(|held| held.is(holder))
}

/// Whether the number field known by `id` has the arrow keys.
pub fn holds_number(id: &str, context: &App) -> bool {
    matches!(held(context), Some(ArrowHolder::Number { id: held, .. }) if *held == id)
}

/// No control has the arrow keys any more.
pub fn release(context: &mut App) {
    context.set_global(HeldArrows(None));
}

/// The control holding the arrow keys.
pub fn held(context: &App) -> Option<&ArrowHolder> {
    context
        .try_global::<HeldArrows>()
        .and_then(|held| held.0.as_ref())
}

impl Tonique {
    /// Steps the control holding the arrow keys by `keystroke`, when it's
    /// an arrow and the keyboard is the app's. Returns whether it did.
    pub(super) fn arrow_key(
        &mut self,
        keystroke: &Keystroke,
        window: &Window,
        context: &mut Context<Self>,
    ) -> bool {
        let (Some(presses), Some(holder)) = (arrow_press(keystroke), held(context).cloned()) else {
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
            // The field's view may be this one, being updated: step it
            // after.
            ArrowHolder::Number { step, .. } => {
                context.defer(move |context| step(presses, context))
            }
        }
        true
    }
}
