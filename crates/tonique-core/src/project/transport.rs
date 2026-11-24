//! Tempo, time signature, playhead, transport, loop and metronome.

use tonique_engine::{
    edit::{
        ClipContent, WarpMode,
        commands::{ResizeClip, SetTempoMap},
    },
    time::{BeatPos, TimeSignature},
};

use super::{PlaybackState, ProjectState, report};

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
        let result = self.edit_session.seek(beats);
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
        let result = self.edit_session.stop();
        report(result);
        self.seek_playhead(self.transport.edit_cursor);
    }
    pub fn play(&mut self) {
        self.transport.playback = PlaybackState::Playing;
        self.pause_preview();
        let result = self.edit_session.play();
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
        report(self.edit_session.set_loop(self.transport.engine_loop()));
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
