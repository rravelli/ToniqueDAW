//! What can be dragged across panels.

use crate::{analysis::AudioInfo, core::effect::EffectKind};

/// Dragged from the browser onto the timeline, a track or the effects
/// panel.
#[derive(Clone)]
pub enum DragPayload {
    /// An audio file, to place as a clip.
    File(AudioInfo),
    /// An effect, to add to a channel.
    Effect(EffectKind),
}
