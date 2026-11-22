//! The value control holding the arrow keys, as the egui app's focused
//! one: clicked, a control holds them and each press steps it (a big step
//! with Shift), as one undo step; Esc or a press anywhere else lets go.
//! The other shortcuts still run meanwhile.

use std::rc::Rc;

use gpui::{App, Context, Global, Keystroke, WeakEntity, Window};
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

/// A stepper calling `step` on `view`, then redrawing it.
pub fn stepper<V: 'static>(
    view: WeakEntity<V>,
    step: impl Fn(&mut V, Presses, &mut Context<V>) + 'static,
) -> Stepper {
    Rc::new(move |presses, context| {
        let _ = view.update(context, |view, context| {
            step(view, presses, context);
            context.notify();
        });
    })
}

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
    /// A number field known by its id, in whichever view.
    Number(&'static str),
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
            (Self::Number(id), Self::Number(other)) => id == other,
            _ => false,
        }
    }
}

/// The control holding the arrow keys, if any, app-wide: any view's
/// control can take them. With a stepper, its view steps it; without, the
/// window does.
#[derive(Default)]
struct HeldArrows(Option<(ArrowHolder, Option<Stepper>)>);

impl Global for HeldArrows {}

/// `holder`, one of the window's controls, takes the arrow keys, from
/// whichever had them.
pub fn hold(holder: ArrowHolder, context: &mut App) {
    context.set_global(HeldArrows(Some((holder, None))));
}

/// `holder`, a control of a view's stepped by `step`, takes the arrow
/// keys.
pub fn hold_in(holder: ArrowHolder, step: Stepper, context: &mut App) {
    context.set_global(HeldArrows(Some((holder, Some(step)))));
}

/// Whether `holder` has the arrow keys: outlined in the accent.
pub fn holds(holder: &ArrowHolder, context: &App) -> bool {
    held(context).is_some_and(|held| held.is(holder))
}

/// Whether the number field known by `id` has the arrow keys.
pub fn holds_number(id: &str, context: &App) -> bool {
    matches!(held(context), Some(ArrowHolder::Number(held)) if *held == id)
}

/// No control has the arrow keys any more.
pub fn release(context: &mut App) {
    context.set_global(HeldArrows(None));
}

/// The control holding the arrow keys.
pub fn held(context: &App) -> Option<&ArrowHolder> {
    held_with_stepper(context).map(|(holder, _)| holder)
}

fn held_with_stepper(context: &App) -> Option<&(ArrowHolder, Option<Stepper>)> {
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
        let (Some(presses), Some((holder, step))) =
            (arrow_press(keystroke), held_with_stepper(context).cloned())
        else {
            return false;
        };
        if !self.has_keyboard(window, context) {
            return false;
        }
        // A view's control: stepped by its view, after this (it may be
        // this one, being updated).
        if let Some(step) = step {
            context.defer(move |context| step(presses, context));
            return true;
        }
        match holder {
            ArrowHolder::Header(track, param) => self.step_header(track, param, presses, context),
            // Stepped by their views.
            ArrowHolder::Clip(_) | ArrowHolder::Knob(_) | ArrowHolder::Number(_) => {}
            ArrowHolder::MetronomeLevel => self.step_metronome_level(presses, context),
        }
        true
    }
}
