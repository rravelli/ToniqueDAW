//! Tempo, time signature, playhead, transport, loop and metronome.

use tonique_engine::{
    edit::{
        ClipContent, WarpMode,
        commands::{ResizeClip, SetTempoMap},
    },
    time::{BeatPos, TimeSignature},
};

use crate::core::grid::GridService;

use super::{DEFAULT_LOOP_BARS, PlaybackState, ProjectState, report};

/// Where playback is and what it does: the playhead, the edit cursor, the
/// loop and the metronome. [`ProjectState`] hands its changes to the
/// engine.
pub(super) struct Transport {
    /// Where the transport is, in beats (follows the engine while playing).
    pub(super) playhead: BeatPos,
    /// Where the user last clicked in the arrangement, in beats. Playback
    /// starts and returns here, and edits (cuts) happen here.
    pub(super) edit_cursor: BeatPos,
    pub(super) playback: PlaybackState,
    /// Whether the timeline scrolls to keep the playhead in view.
    pub(super) follow_playhead: bool,
    pub(super) metronome: bool,
    /// Loop region in beats; playback wraps over it while `looping`.
    pub(super) loop_range: (BeatPos, BeatPos),
    pub(super) looping: bool,
}

impl Transport {
    pub(super) fn new() -> Self {
        Self {
            playhead: BeatPos::ZERO,
            edit_cursor: BeatPos::ZERO,
            playback: PlaybackState::Paused,
            follow_playhead: false,
            metronome: false,
            loop_range: Self::default_loop(GridService::new().beats_per_bar()),
            looping: false,
        }
    }

    /// The loop region of a new project, with `beats_per_bar`.
    pub(super) fn default_loop(beats_per_bar: f64) -> (BeatPos, BeatPos) {
        (BeatPos::ZERO, BeatPos(DEFAULT_LOOP_BARS * beats_per_bar))
    }

    /// Back to the start of a new project, not looping over its first
    /// bars. The metronome and following the playhead are preferences:
    /// kept.
    pub(super) fn reset(&mut self, beats_per_bar: f64) {
        self.playhead = BeatPos::ZERO;
        self.edit_cursor = BeatPos::ZERO;
        self.loop_range = Self::default_loop(beats_per_bar);
        self.looping = false;
    }

    pub(super) fn is_playing(&self) -> bool {
        self.playback == PlaybackState::Playing
    }

    /// What the engine loops over: the region, while looping.
    pub(super) fn engine_loop(&self) -> Option<(BeatPos, BeatPos)> {
        self.looping.then_some(self.loop_range)
    }

    /// Sets the loop region between `a` and `b`, in any order and not
    /// before the start; whether it changed (empty regions are ignored).
    pub(super) fn set_loop_between(&mut self, a: BeatPos, b: BeatPos) -> bool {
        let (start, end) = (a.min(b).max(BeatPos::ZERO), a.max(b).max(BeatPos::ZERO));
        if end <= start || (start, end) == self.loop_range {
            return false;
        }
        self.loop_range = (start, end);
        true
    }

    /// The metronome's level: `level` while it's on.
    pub(super) fn metronome_gain(&self, level: f32) -> f32 {
        if self.metronome { level } else { 0. }
    }
}

impl ProjectState {
    pub fn set_bpm(&mut self, value: f32) {
        let old = self.bpm();
        if value <= 0. || value == old {
            return;
        }
        // Unwarped audio clips keep their length in seconds, so their length
        // in beats scales with the tempo; warped ones keep theirs in beats.
        let resized: Vec<_> = self
            .edit()
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(move |c| (t.id, c)))
            .filter_map(|(t, c)| match c.content {
                ClipContent::Audio {
                    source_offset_s,
                    warp: WarpMode::Off,
                    ..
                } => {
                    let length = c.length * value as f64 / old as f64;
                    Some(ResizeClip::new(t, c.id, c.start, length, source_offset_s))
                }
                ClipContent::Audio { .. } | ClipContent::Midi { .. } => None,
            })
            .collect();
        let mut tempo = self.edit().tempo.clone();
        tempo.set_tempo(BeatPos(0.), value as f64);
        self.transaction("Change tempo", |s| {
            s.perform(SetTempoMap::new(tempo));
            for cmd in resized {
                s.perform(cmd);
            }
        });
    }

    pub fn bpm(&self) -> f32 {
        self.edit().tempo.bpm_at(BeatPos(0.)) as f32
    }
    // Time signature
    /// Change the project's time signature. Positions are in quarter-note
    /// beats, so clips stay where they are; only the bars move.
    pub fn set_time_signature(&mut self, signature: TimeSignature) {
        if signature == self.time_signature() {
            return;
        }
        let mut tempo = self.edit().tempo.clone();
        tempo.time_signature = signature;
        self.transaction("Change time signature", |s| {
            s.perform(SetTempoMap::new(tempo))
        });
        self.sync_time_signature();
    }

    pub fn time_signature(&self) -> TimeSignature {
        self.edit().tempo.time_signature
    }

    /// The grid keeps a copy of the signature: refresh it after anything
    /// that may change the edit's (edits, undo, redo, loading).
    pub(super) fn sync_time_signature(&mut self) {
        let signature = self.time_signature();
        self.grid.set_time_signature(signature);
    }
    // Playhead and edit cursor
    /// Move the playhead and the edit cursor there (ruler, playhead handle).
    pub fn seek(&mut self, beats: BeatPos) {
        self.transport.edit_cursor = beats.max(BeatPos::ZERO);
        self.seek_playhead(self.transport.edit_cursor);
    }
    /// Place the edit cursor (a click in the arrangement). Doesn't interrupt
    /// playback; when stopped, the playhead follows so play starts there.
    pub fn set_edit_cursor(&mut self, beats: BeatPos) {
        self.transport.edit_cursor = beats.max(BeatPos::ZERO);
        if !self.transport.is_playing() {
            self.seek_playhead(self.transport.edit_cursor);
        }
    }
    fn seek_playhead(&mut self, beats: BeatPos) {
        self.transport.playhead = beats;
        let result = self.session.seek(beats);
        report(result);
    }
    pub fn playhead(&self) -> BeatPos {
        self.transport.playhead
    }
    pub fn edit_cursor(&self) -> BeatPos {
        self.transport.edit_cursor
    }
    pub fn follow_playhead(&self) -> bool {
        self.transport.follow_playhead
    }
    pub fn set_follow_playhead(&mut self, follow: bool) {
        self.transport.follow_playhead = follow;
    }
    // Transport state
    /// Stop playback and return the playhead to the edit cursor.
    pub fn stop(&mut self) {
        self.transport.playback = PlaybackState::Paused;
        let result = self.session.stop();
        report(result);
        self.seek_playhead(self.transport.edit_cursor);
    }
    pub fn play(&mut self) {
        self.transport.playback = PlaybackState::Playing;
        self.pause_preview();
        let result = self.session.play();
        report(result);
    }
    // Loop
    pub fn looping(&self) -> bool {
        self.transport.looping
    }
    pub fn set_looping(&mut self, looping: bool) {
        self.transport.looping = looping;
        self.sync_loop();
    }
    pub fn loop_range(&self) -> (BeatPos, BeatPos) {
        self.transport.loop_range
    }
    /// Set the loop region (in any order). Empty regions are ignored.
    pub fn set_loop_range(&mut self, a: BeatPos, b: BeatPos) {
        if self.transport.set_loop_between(a, b) {
            self.sync_loop();
        }
    }
    /// Hand the loop to the engine, in beats: it keeps it on them through
    /// tempo changes, as it does the playhead.
    pub(super) fn sync_loop(&mut self) {
        report(self.session.set_loop(self.transport.engine_loop()));
    }
    pub fn toggle_metronome(&mut self) {
        self.transport.metronome = !self.transport.metronome;
        self.apply_metronome_level();
    }
    pub(super) fn apply_metronome_level(&self) {
        let gain = self.transport.metronome_gain(self.settings.metronome_level);
        self.edit().metronome.set(gain, 0.);
    }
    pub fn metronome(&self) -> bool {
        self.transport.metronome
    }

    pub fn playback_state(&self) -> PlaybackState {
        self.transport.playback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loops_are_set_in_any_order_and_never_empty() {
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
    fn a_reset_keeps_the_preferences() {
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
