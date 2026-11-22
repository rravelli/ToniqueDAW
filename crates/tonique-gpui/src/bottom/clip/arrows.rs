//! The inspector's values stepped by the arrow keys once clicked: a slider
//! (0.5 dB, a fiftieth of a side, a semitone or a cent; 6 dB, ten, an
//! octave or 10 cents with Shift, the detune carrying over into the
//! semitones), a position or length (a sixteenth, a bar with Shift), or a
//! time in the file (10 ms, a second with Shift). Each press is an undo
//! step.

use crate::bottom::BottomPanel;
use crate::model::HoldsProject;
use crate::{
    bottom::clip::{audio::ClipSlider, inspector::Field},
    utils::steps::Presses,
};
use gpui::App;

/// A value of the inspector's holding the arrow keys, of the clip shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClipArrows {
    Slider(ClipSlider),
    Field(Field),
}

impl BottomPanel {
    pub(crate) fn step_clip(&mut self, arrows: ClipArrows, presses: Presses, context: &mut App) {
        if self.clip.drag.is_some() {
            return;
        }
        let Some((_, clip)) = self
            .clip
            .clip
            .and_then(|id| self.state(context).clip_with_track(id))
        else {
            return;
        };
        match arrows {
            ClipArrows::Slider(slider) => self.step_clip_slider(&clip, slider, presses, context),
            ClipArrows::Field(field) => self.step_field(&clip, field, presses, context),
        }
    }
}
