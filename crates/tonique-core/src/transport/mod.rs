//! Where playback is and what it does: the playhead, the edit cursor, the
//! loop and the metronome, and the browser's preview. The project hands
//! their changes to the engine.

pub mod preview;

use tonique_engine::time::{BeatPos, TimeSignature};

/// Length of the loop region in a new project.
const DEFAULT_LOOP_BARS: f64 = 4.;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaybackState {
    Paused,
    Playing,
}

/// Where playback is and what it does: the playhead, the edit cursor, the
/// loop and the metronome.
pub(crate) struct Transport {
    /// Where the transport is, in beats (follows the engine while playing).
    pub(crate) playhead: BeatPos,
    /// Where the user last clicked in the arrangement, in beats. Playback
    /// starts and returns here, and edits (cuts) happen here.
    pub(crate) edit_cursor: BeatPos,
    pub(crate) playback: PlaybackState,
    /// Whether the timeline scrolls to keep the playhead in view.
    pub(crate) follow_playhead: bool,
    pub(crate) metronome: bool,
    /// Loop region in beats; playback wraps over it while `looping`.
    pub(crate) loop_range: (BeatPos, BeatPos),
    pub(crate) looping: bool,
}

impl Transport {
    pub(crate) fn new() -> Self {
        Self {
            playhead: BeatPos::ZERO,
            edit_cursor: BeatPos::ZERO,
            playback: PlaybackState::Paused,
            follow_playhead: false,
            metronome: false,
            loop_range: Self::default_loop(TimeSignature::default().beats_per_bar()),
            looping: false,
        }
    }

    /// The loop region of a new project, with `beats_per_bar`.
    pub(crate) fn default_loop(beats_per_bar: f64) -> (BeatPos, BeatPos) {
        (BeatPos::ZERO, BeatPos(DEFAULT_LOOP_BARS * beats_per_bar))
    }

    /// Back to the start of a new project, not looping over its first
    /// bars. The metronome and following the playhead are preferences:
    /// kept.
    pub(crate) fn reset(&mut self, beats_per_bar: f64) {
        self.playhead = BeatPos::ZERO;
        self.edit_cursor = BeatPos::ZERO;
        self.loop_range = Self::default_loop(beats_per_bar);
        self.looping = false;
    }

    pub(crate) fn is_playing(&self) -> bool {
        self.playback == PlaybackState::Playing
    }

    /// What the engine loops over: the region, while looping.
    pub(crate) fn engine_loop(&self) -> Option<(BeatPos, BeatPos)> {
        self.looping.then_some(self.loop_range)
    }

    /// Sets the loop region between `a` and `b`, in any order and not
    /// before the start; whether it changed (empty regions are ignored).
    pub(crate) fn set_loop_between(&mut self, a: BeatPos, b: BeatPos) -> bool {
        let (start, end) = (a.min(b).max(BeatPos::ZERO), a.max(b).max(BeatPos::ZERO));
        if end <= start || (start, end) == self.loop_range {
            return false;
        }
        self.loop_range = (start, end);
        true
    }

    /// The metronome's level: `level` while it's on.
    pub(crate) fn metronome_gain(&self, level: f32) -> f32 {
        if self.metronome { level } else { 0. }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub(crate) fn loops_are_set_in_any_order_and_never_empty() {
        let mut transport = Transport::new();
        assert!(transport.set_loop_between(BeatPos(8.), BeatPos(2.)));
        assert_eq!(transport.loop_range, (BeatPos(2.), BeatPos(8.)));
        assert!(
            !transport.set_loop_between(BeatPos(2.), BeatPos(8.)),
            "unchanged"
        );
        assert!(
            !transport.set_loop_between(BeatPos(-4.), BeatPos(-1.)),
            "empty"
        );
        assert_eq!(transport.engine_loop(), None, "not looping");
        transport.looping = true;
        assert_eq!(transport.engine_loop(), Some((BeatPos(2.), BeatPos(8.))));
    }

    #[test]
    pub(crate) fn a_reset_keeps_the_preferences() {
        let mut transport = Transport::new();
        transport.metronome = true;
        transport.follow_playhead = true;
        transport.looping = true;
        transport.playhead = BeatPos(9.);
        transport.reset(3.);
        assert_eq!(transport.playhead, BeatPos::ZERO);
        assert_eq!(transport.loop_range, (BeatPos::ZERO, BeatPos(12.)));
        assert!(!transport.looping);
        assert!(transport.metronome && transport.follow_playhead);
    }
}
