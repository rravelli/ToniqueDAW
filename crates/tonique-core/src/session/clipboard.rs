//! Copied clips, for pasting.

use crate::document::clip::AudioClip;

/// Copied clips, placed relative to the copied range: time from its start,
/// tracks from the topmost copied one.
#[derive(Debug, Clone, Default)]
pub struct Clipboard {
    pub(crate) clips: Vec<(usize, AudioClip)>,
    /// Length of the copied range, in beats.
    pub(crate) length: f64,
    /// Index of the topmost copied track, to paste back there when no
    /// track is selected.
    pub(crate) top_track: usize,
    /// The tempo then: unwarped clips keep their length in seconds, so
    /// pasted at another, their length in beats changes (as
    /// [`ProjectState::set_bpm`] does to those in the arrangement).
    pub(crate) bpm: f32,
}
