//! What can be dragged across panels.

use crate::{
    analysis::AudioInfo,
    core::{effect::EffectKind, state::ProjectState},
};
use tonique_engine::edit::{PluginId, TrackId};

/// Dragged from the browser onto the timeline, a track or the effects
/// panel, or from the effects panel onto a track.
#[derive(Clone)]
pub enum DragPayload {
    /// An audio file, to place as a clip.
    File(AudioInfo),
    /// An effect, to add to a channel.
    Effect(EffectKind),
    /// An effect on `track`, to move along its chain or to another channel.
    MovedEffect { track: TrackId, plugin: PluginId },
}

impl DragPayload {
    /// Whether dropping this on `track` puts an effect on it.
    pub fn puts_effect_on(&self, track: TrackId) -> bool {
        match self {
            Self::File(_) => false,
            Self::Effect(_) => true,
            Self::MovedEffect { track: from, .. } => *from != track,
        }
    }

    /// Put the effect this carries at the end of `track`'s chain.
    pub fn drop_effect(&self, state: &mut ProjectState, track: TrackId) {
        match self {
            Self::File(_) => {}
            Self::Effect(kind) => state.add_effect(&track, *kind, usize::MAX),
            Self::MovedEffect {
                track: from,
                plugin,
            } => state.move_effect_to_track(from, *plugin, &track),
        }
    }
}
