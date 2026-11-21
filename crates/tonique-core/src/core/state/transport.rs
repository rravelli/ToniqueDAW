//! Tempo, time signature, playhead, transport, loop, metronome and the
//! browser's preview.

use crate::audio::preview::FilePreview;
use std::path::PathBuf;

use tonique_engine::{
    edit::{
        ClipContent, EditError, WarpMode,
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
        self.edit_cursor = beats.max(BeatPos::ZERO);
        self.seek_playhead(self.edit_cursor);
    }
    /// Place the edit cursor (a click in the arrangement). Doesn't interrupt
    /// playback; when stopped, the playhead follows so play starts there.
    pub fn set_edit_cursor(&mut self, beats: BeatPos) {
        self.edit_cursor = beats.max(BeatPos::ZERO);
        if self.playback_state != PlaybackState::Playing {
            self.seek_playhead(self.edit_cursor);
        }
    }
    fn seek_playhead(&mut self, beats: BeatPos) {
        self.playhead = beats;
        let result = self.session.seek(beats);
        report(result);
    }
    pub fn playhead(&self) -> BeatPos {
        self.playhead
    }
    pub fn edit_cursor(&self) -> BeatPos {
        self.edit_cursor
    }
    pub fn follow_playhead(&self) -> bool {
        self.follow_playhead
    }
    pub fn set_follow_playhead(&mut self, follow: bool) {
        self.follow_playhead = follow;
    }
    // Transport state
    /// Stop playback and return the playhead to the edit cursor.
    pub fn stop(&mut self) {
        self.playback_state = PlaybackState::Paused;
        let result = self.session.stop();
        report(result);
        self.seek_playhead(self.edit_cursor);
    }
    pub fn play(&mut self) {
        self.playback_state = PlaybackState::Playing;
        self.pause_preview();
        let result = self.session.play();
        report(result);
    }
    pub fn pause_preview(&mut self) {
        self.preview_playback_state = PlaybackState::Paused;
        let result = self.session.engine_mut().preview_stop();
        report(result.map_err(EditError::from));
    }
    /// Play a file from the start. Ignored while the transport plays.
    pub fn play_preview(&mut self, path: PathBuf) {
        if self.playback_state == PlaybackState::Playing {
            return;
        }
        let Some(source) = FilePreview::open(path.clone()) else {
            return;
        };
        self.preview_position = 0;
        self.preview_path = Some(path.clone());
        self.preview_playback_state = PlaybackState::Playing;
        let engine = self.session.engine_mut();
        engine.preview_set_gain(self.preview_gain);
        let result = engine.preview_play(Box::new(source), 0);
        report(result.map_err(EditError::from));
    }
    /// How loud the preview plays (linear), now and next time.
    pub fn set_preview_gain(&mut self, gain: f32) {
        self.preview_gain = gain;
        self.session.engine_mut().preview_set_gain(gain);
    }
    /// Continue the last previewed file from `pos` (in frames of the file).
    pub fn seek_preview(&mut self, pos: usize) {
        self.preview_position = pos;
        self.preview_playback_state = PlaybackState::Playing;
        let result = self.session.engine_mut().preview_seek(pos);
        report(result.map_err(EditError::from));
    }
    // Loop
    pub fn looping(&self) -> bool {
        self.looping
    }
    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
        self.sync_loop();
    }
    pub fn loop_range(&self) -> (BeatPos, BeatPos) {
        self.loop_range
    }
    /// Set the loop region (in any order). Empty regions are ignored.
    pub fn set_loop_range(&mut self, a: BeatPos, b: BeatPos) {
        let (start, end) = (a.min(b).max(BeatPos::ZERO), a.max(b).max(BeatPos::ZERO));
        if end > start && (start, end) != self.loop_range {
            self.loop_range = (start, end);
            self.sync_loop();
        }
    }
    /// Hand the loop to the engine, in beats: it keeps it on them through
    /// tempo changes, as it does the playhead.
    pub(super) fn sync_loop(&mut self) {
        let range = self.looping.then_some(self.loop_range);
        report(self.session.set_loop(range));
    }
    pub fn toggle_metronome(&mut self) {
        self.metronome = !self.metronome;
        self.apply_metronome_level();
    }
    pub(super) fn apply_metronome_level(&self) {
        let gain = if self.metronome {
            self.settings.metronome_level
        } else {
            0.
        };
        self.edit().metronome.set(gain, 0.);
    }
    pub fn metronome(&self) -> bool {
        self.metronome
    }

    pub fn playback_state(&self) -> PlaybackState {
        self.playback_state
    }
    pub fn preview_playback_state(&self) -> PlaybackState {
        self.preview_playback_state
    }
    pub fn preview_position(&self) -> usize {
        self.preview_position
    }
    /// The file the preview is playing, if it is.
    pub fn previewing(&self) -> Option<&std::path::Path> {
        self.preview_path
            .as_deref()
            .filter(|_| self.preview_playback_state == PlaybackState::Playing)
    }
}
