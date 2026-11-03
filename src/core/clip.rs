use crate::analysis::AudioInfo;
use std::{collections::HashMap, fmt::Debug, ops::Range};
use tonique_engine::{
    edit::{
        ClipFades, ClipId, ClipLoop, Fade, MAX_SPEED, MAX_TRANSPOSE, MIN_SPEED, SeamRoom,
        StretchSettings, WarpMode, seams,
    },
    nodes::{ClipWarp, FadeCurve},
    time::BeatPos,
};

/// Most pieces [`AudioClip::segments`] cuts a stretch into: a tiny loop
/// over a long stretch would make too many to draw.
const MAX_SEGMENTS: usize = 4096;
/// Shortest a clip, or a loop, can be, in beats.
pub const MIN_LENGTH: f64 = 1. / 16.;
/// How far a clip is detuned, in cents: half a semitone down to just short
/// of half up, so a pitch splits into semitones and cents one way only
/// (see [`split_pitch`]).
pub const MIN_DETUNE: f32 = -50.;
pub const MAX_DETUNE: f32 = 49.;

/// A pitch in semitones (fractions being cents) as whole semitones and
/// cents from [`MIN_DETUNE`] to [`MAX_DETUNE`]: the nearest semitone (the
/// one above, half way), and what's left.
pub fn split_pitch(pitch: f32) -> (i32, f32) {
    let semitones = (pitch + 0.5).floor();
    (semitones as i32, (pitch - semitones) * 100.)
}

/// `semitones` and `cents`, the cents past the detune's range carried over
/// into semitones (`0` and `+50` are `+1` and `-50`), no further than
/// [`MAX_TRANSPOSE`] either way. Exact for whole cents.
pub fn carried(semitones: i32, cents: f32) -> (i32, f32) {
    let limit = MAX_TRANSPOSE * 100.;
    let cents = (semitones as f32 * 100. + cents).clamp(-limit, limit);
    let semitones = ((cents - MIN_DETUNE) / 100.).floor();
    (semitones as i32, cents - semitones * 100.)
}

/// A clip representing an audio file placed on a track: from `offset` in
/// the file (its start marker), for `length` beats; with a loop, once
/// reading reaches the loop's end it goes on from its start, as long as the
/// clip lasts.
#[derive(Clone)]
pub struct AudioClip {
    pub id: ClipId,
    /// Audio metadata
    pub audio: AudioInfo,
    pub position: BeatPos,
    /// In the arrangement, in beats. Without a loop, no longer than what's
    /// left of the file after `offset`.
    pub length: f64,
    /// Where in the file playback starts, in seconds.
    pub offset: f64,
    /// The part of the file that loops, in seconds.
    pub looped: Option<ClipLoop>,
    /// Linear, 1 to play the file as is.
    pub gain: f32,
    /// Balance, -1 (left only) to 1 (right only).
    pub pan: f32,
    /// Where bar 1 of the clip's own grid is in the file, in seconds: the
    /// file's start unless set. Trims and slips leave it where it is.
    pub origin: f64,
    /// How it follows the project's tempo.
    pub warp: WarpMode,
    /// The file's tempo: warped, the clip plays at the project's over it.
    pub bpm: f64,
    /// In whole semitones: warped, the pitch alone moves; unwarped, the
    /// speed too.
    pub transpose: i32,
    /// Fine tuning, in cents ([`MIN_DETUNE`] to [`MAX_DETUNE`]), as
    /// `transpose`.
    pub detune: f32,
    /// How it's stretched, warped.
    pub stretch: StretchSettings,
    /// How it fades in and out, and crosses the clip before it.
    pub fades: ClipFades,
}

/// Where two clips of a track touch, a crossfade possible across it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seam {
    pub earlier: ClipId,
    pub later: ClipId,
    /// Where they touch, in beats.
    pub at: BeatPos,
    pub room: SeamRoom,
    /// How far its crossfade reaches before and after it, in seconds: none
    /// without one.
    pub before_s: f64,
    pub after_s: f64,
    /// The later clip's fade in; the earlier one's is
    /// [`crossing`](tonique_engine::nodes::FadeCurve::crossing) it.
    pub curve: FadeCurve,
}

impl Seam {
    pub fn length_s(&self) -> f64 {
        self.before_s + self.after_s
    }
}

/// The seams between `clips`, those of a track.
pub fn clip_seams(clips: &[AudioClip], bpm: f32) -> Vec<Seam> {
    let spans: Vec<_> = clips.iter().map(|c| (c.position.0, c.end(bpm).0)).collect();
    clips
        .iter()
        .zip(seams(&spans))
        .filter_map(|(later, earlier)| {
            let earlier = &clips[earlier?];
            let room = SeamRoom::new(
                later.spare_before(bpm),
                earlier.spare_after(bpm),
                earlier.seconds(bpm),
                later.seconds(bpm),
            );
            let crossfade = later.fades.crossfade;
            let (before_s, after_s) = room.window(crossfade.length_s);
            Some(Seam {
                earlier: earlier.id,
                later: later.id,
                at: later.position,
                room,
                before_s,
                after_s,
                curve: crossfade.curve,
            })
        })
        .collect()
}

/// What's at a clip's ends: whether another clip touches it there, and the
/// fade of its crossfade across that if there's one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Crossfades {
    pub start_seam: bool,
    pub end_seam: bool,
    pub start: Option<Fade>,
    pub end: Option<Fade>,
}

/// What's at the ends of each clip of `seams` that has a seam.
pub fn crossfades(seams: &[Seam]) -> HashMap<ClipId, Crossfades> {
    let mut found: HashMap<ClipId, Crossfades> = HashMap::new();
    for seam in seams {
        let fade = |curve| {
            (seam.length_s() > 0.).then_some(Fade {
                length_s: seam.length_s(),
                curve,
            })
        };
        let later = found.entry(seam.later).or_default();
        later.start_seam = true;
        later.start = fade(seam.curve);
        let earlier = found.entry(seam.earlier).or_default();
        earlier.end_seam = true;
        earlier.end = fade(seam.curve.crossing());
    }
    found
}

/// A stretch of a clip playing one stretch of its file, without a jump.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    /// From the clip's start, in beats.
    pub start: f64,
    pub end: f64,
    /// In the file, in seconds: where `start` and `end` read.
    pub file_start: f64,
    pub file_end: f64,
}

impl AudioClip {
    /// The whole file, at `position`. Get `id` from
    /// `ProjectState::new_clip_id`; `bpm` gives the file's length in beats.
    pub fn new(id: ClipId, audio: AudioInfo, position: BeatPos, bpm: f32) -> Self {
        let mut clip = Self {
            id,
            audio,
            position,
            length: 0.,
            offset: 0.,
            looped: None,
            gain: 1.,
            pan: 0.,
            origin: 0.,
            warp: WarpMode::Spectral,
            bpm: bpm as f64,
            transpose: 0,
            detune: 0.,
            stretch: StretchSettings::default(),
            fades: ClipFades::default(),
        };
        clip.length = clip.source_beats(bpm);
        clip
    }

    /// A copy under `id`. A crossfade is across a seam, which a copy isn't
    /// on: it has none.
    pub fn with_id(&self, id: ClipId) -> Self {
        let mut clone = self.clone();
        clone.id = id;
        clone.fades.crossfade.length_s = 0.;
        clone
    }

    /// Length of the whole source file, in seconds.
    pub fn source_seconds(&self) -> f32 {
        self.audio.duration.map_or(0., |d| d.as_secs_f32())
    }

    /// Its transposition in semitones, fractions being cents.
    pub fn pitch(&self) -> f32 {
        self.transpose as f32 + self.detune / 100.
    }

    /// How it plays its file at `bpm`.
    pub fn play(&self, bpm: f32) -> ClipWarp {
        self.warp
            .play(self.bpm, self.pitch(), self.stretch, bpm as f64)
    }

    /// Seconds of the file played per second at `bpm`.
    pub fn speed(&self, bpm: f32) -> f64 {
        self.play(bpm).speed()
    }

    /// Beats the clip takes to play `seconds` of its file.
    pub fn file_beats(&self, seconds: f64, bpm: f32) -> f64 {
        to_beats(seconds / self.speed(bpm), bpm)
    }

    /// Seconds of its file the clip plays in `beats`.
    pub fn file_seconds(&self, beats: f64, bpm: f32) -> f64 {
        to_seconds(beats, bpm) * self.speed(bpm)
    }

    /// Follow the tempo as `warp` with the file at `file_bpm`, transposed by
    /// `transpose` semitones and `detune` cents, keeping what's played of
    /// the file: the clip gets shorter played faster, longer played slower
    /// (no longer than the file allows).
    pub fn set_warp(
        &mut self,
        warp: WarpMode,
        file_bpm: f64,
        transpose: i32,
        detune: f32,
        bpm: f32,
    ) {
        let played = self.file_seconds(self.length, bpm);
        let limit = MAX_TRANSPOSE as i32;
        self.warp = warp;
        if file_bpm > 0. {
            self.bpm = file_bpm;
        }
        self.transpose = transpose.clamp(-limit, limit);
        self.detune = detune.clamp(MIN_DETUNE, MAX_DETUNE);
        self.length = self
            .file_beats(played, bpm)
            .max(MIN_LENGTH)
            .min(self.max_length(bpm));
    }

    /// Length of the whole source file as the clip plays it, in beats.
    pub fn source_beats(&self, bpm: f32) -> f64 {
        self.file_beats(self.source_seconds() as f64, bpm)
    }

    /// The loop, if it's one that plays: inside the file, and not empty.
    pub fn active_loop(&self) -> Option<ClipLoop> {
        let source = self.source_seconds() as f64;
        self.looped
            .filter(|l| l.is_valid() && l.end_s <= source + 1e-9)
    }

    /// Where in the file the clip reads `beats` after its start, in
    /// seconds.
    pub fn file_at(&self, beats: f64, bpm: f32) -> f64 {
        let at = self.offset + self.file_seconds(beats, bpm);
        match self.active_loop() {
            Some(l) if at >= l.end_s => {
                l.start_s + (at - l.start_s).rem_euclid(l.end_s - l.start_s)
            }
            _ => at,
        }
    }

    /// Longest the clip can be: to the file's end, or for ever with a loop.
    pub fn max_length(&self, bpm: f32) -> f64 {
        if self.active_loop().is_some() {
            f64::INFINITY
        } else {
            self.file_beats(self.source_seconds() as f64 - self.offset, bpm)
                .max(0.)
        }
    }

    /// Lengths are kept in beats: `_bpm` only for symmetry with MIDI clips.
    pub fn end(&self, _bpm: f32) -> BeatPos {
        self.position + self.length
    }

    /// Where the start of the file lies in the arrangement, read without
    /// looping: before the clip's position by its offset.
    pub fn file_start(&self, bpm: f32) -> BeatPos {
        self.position - self.file_beats(self.offset, bpm)
    }

    /// Move the clip's start to `beats`, keeping its end and its sound in
    /// place: what's left of the start goes, keeping [`MIN_LENGTH`];
    /// extending it reveals what played before, no earlier than the first
    /// beat, nor (without a loop) the start of the file. A loop goes on
    /// repeating backwards. Does nothing when the file's length is unknown.
    pub fn trim_start_at(&mut self, beats: BeatPos, bpm: f32) {
        if self.source_seconds() <= 0. {
            return;
        }
        let end = self.end(bpm);
        let mut beats = beats.min(end - MIN_LENGTH).max(BeatPos::ZERO);
        let looping_back = self.active_loop().filter(|l| self.offset >= l.start_s);
        if beats < self.position && looping_back.is_none() {
            // Not before the file's start.
            beats = beats.max(self.file_start(bpm));
        }
        let shift = beats - self.position;
        self.offset = match looping_back {
            Some(l) if shift < 0. => {
                let back = self.offset - l.start_s + self.file_seconds(shift, bpm);
                l.start_s + back.rem_euclid(l.end_s - l.start_s)
            }
            _ => self.file_at(shift, bpm),
        };
        self.position = beats;
        self.length = end - beats;
    }

    /// Move the clip's end to `beats`, keeping its start and
    /// [`MIN_LENGTH`]: no further than the file's end, unless it loops.
    /// Does nothing when the file's length is unknown.
    pub fn trim_end_at(&mut self, beats: BeatPos, bpm: f32) {
        if self.source_seconds() <= 0. {
            return;
        }
        self.length = (beats - self.position)
            .max(MIN_LENGTH)
            .min(self.max_length(bpm));
    }

    /// Stretch the clip to end at `beats`, keeping its start: it plays the
    /// same part of its file, faster or slower. See [`Self::stretch_to`].
    pub fn stretch_end_at(&mut self, beats: BeatPos, bpm: f32) {
        self.stretch_to(beats - self.position, bpm);
    }

    /// Stretch the clip to start at `beats` (not before the first beat),
    /// keeping its end, as [`Self::stretch_end_at`].
    pub fn stretch_start_at(&mut self, beats: BeatPos, bpm: f32) {
        let end = self.end(bpm);
        self.stretch_to(end - beats.max(BeatPos::ZERO), bpm);
        self.position = end - self.length;
    }

    /// Play what the clip plays of its file over `length` beats (at least
    /// [`MIN_LENGTH`]), warped to the project's tempo with its file's
    /// tempo set to fit, its pitch kept. As near as the stretcher's speeds
    /// ([`MIN_SPEED`] to [`MAX_SPEED`]) allow.
    fn stretch_to(&mut self, length: f64, bpm: f32) {
        let played = self.file_seconds(self.length, bpm);
        if played <= 0. {
            return;
        }
        let seconds = to_seconds(length.max(MIN_LENGTH), bpm);
        let speed = (played / seconds).clamp(MIN_SPEED, MAX_SPEED);
        self.warp = WarpMode::Spectral;
        self.bpm = bpm as f64 / speed;
        self.length = self.file_beats(played, bpm);
    }

    /// Keep only the part of the clip inside `start..end` (in beats). Never
    /// reveals audio that was trimmed away. A fade stays only at an end that
    /// stays.
    pub fn crop(&mut self, start: BeatPos, end: BeatPos, bpm: f32) {
        let clip_end = self.end(bpm);
        if start > self.position {
            self.fades.fade_in.length_s = 0.;
            self.fades.crossfade.length_s = 0.;
        }
        if end < clip_end {
            self.fades.fade_out.length_s = 0.;
        }
        self.trim_start_at(start.max(self.position), bpm);
        self.trim_end_at(end.min(clip_end), bpm);
    }

    /// How long it lasts on the timeline, in seconds.
    pub fn seconds(&self, bpm: f32) -> f64 {
        to_seconds(self.length, bpm)
    }

    /// Seconds of the timeline it could start earlier, reading what's
    /// trimmed off its start.
    pub fn spare_before(&self, bpm: f32) -> f64 {
        self.offset / self.speed(bpm)
    }

    /// Seconds of the timeline it could play on past its end, reading
    /// what's trimmed off it: for ever with a loop.
    pub fn spare_after(&self, bpm: f32) -> f64 {
        to_seconds(self.max_length(bpm) - self.length, bpm).max(0.)
    }

    /// Its fades in and out as heard with `crossed`, its crossfades.
    pub fn heard_fades(&self, crossed: Crossfades, bpm: f32) -> (Fade, Fade) {
        self.fades
            .heard(self.seconds(bpm), crossed.start, crossed.end)
    }

    /// Fade in over `seconds`, as far as what's at its end leaves room: its
    /// fade out, as heard from now on, or a crossfade from `crossed`.
    pub fn set_fade_in(&mut self, seconds: f64, crossed: Crossfades, bpm: f32) {
        let start = None;
        let (_, end) = self.heard_fades(Crossfades { start, ..crossed }, bpm);
        if crossed.end.is_none() {
            self.fades.fade_out.length_s = end.length_s;
        }
        let room = (self.seconds(bpm) - end.length_s).max(0.);
        self.fades.fade_in.length_s = seconds.clamp(0., room);
    }

    /// Fade out over `seconds`, as far as what's at its start leaves room:
    /// its fade in, as heard from now on, or a crossfade from `crossed`.
    pub fn set_fade_out(&mut self, seconds: f64, crossed: Crossfades, bpm: f32) {
        let end = None;
        let (start, _) = self.heard_fades(Crossfades { end, ..crossed }, bpm);
        if crossed.start.is_none() {
            self.fades.fade_in.length_s = start.length_s;
        }
        let room = (self.seconds(bpm) - start.length_s).max(0.);
        self.fades.fade_out.length_s = seconds.clamp(0., room);
    }

    /// Start (or stop) looping `looped`. Turning a loop off cuts the clip
    /// at the file's end, its start moving back if that leaves less than
    /// [`MIN_LENGTH`] to play.
    pub fn set_loop(&mut self, looped: Option<ClipLoop>, bpm: f32) {
        self.looped = looped;
        if self.active_loop().is_none() {
            let source = self.source_seconds() as f64;
            let min = self.file_seconds(MIN_LENGTH, bpm).min(source);
            self.offset = self.offset.min(source - min).max(0.);
        }
        self.length = self.length.min(self.max_length(bpm));
    }

    /// Play from `offset` seconds into the file, the clip staying where it
    /// is: within the file, and (without a loop) no further than lets it
    /// play to its end.
    pub fn slip_to(&mut self, offset: f64, bpm: f32) {
        let source = self.source_seconds() as f64;
        let last = match self.active_loop() {
            Some(l) => l.end_s,
            None => source - self.file_seconds(self.length, bpm),
        };
        self.offset = offset.clamp(0., last.max(0.));
    }

    /// The clip cut where it jumps, at each pass of its loop: what to draw.
    /// Only the pieces over `range` (in beats from the clip's start), so a
    /// long clip on a short loop cuts only the passes in view.
    pub fn segments(&self, range: Range<f64>, bpm: f32) -> Vec<Segment> {
        let mut segments = Vec::new();
        let mut start = self.pass_before(range.start, bpm);
        let stop = range.end.min(self.length);
        while start < stop && segments.len() < MAX_SEGMENTS {
            let file_start = self.file_at(start, bpm);
            // Until the loop's end, or the file's.
            let until = match self.active_loop() {
                Some(l) if file_start < l.end_s => l.end_s,
                _ => self.source_seconds() as f64,
            };
            let end = (start + self.file_beats(until - file_start, bpm)).min(self.length);
            if end <= start {
                break;
            }
            segments.push(Segment {
                start,
                end,
                file_start,
                file_end: file_start + self.file_seconds(end - start, bpm),
            });
            start = end;
        }
        segments
    }

    /// Where the pass of the loop playing at `beats` (from the clip's
    /// start) begins: 0 before the first jump. Passes after it all last
    /// the loop's length.
    fn pass_before(&self, beats: f64, bpm: f32) -> f64 {
        let Some(l) = self.active_loop() else {
            return 0.;
        };
        // Reading wraps into the loop, so it starts before its end.
        let first = to_beats(l.end_s - self.file_at(0., bpm), bpm);
        if beats < first {
            return 0.;
        }
        let pass = to_beats(l.end_s - l.start_s, bpm);
        first + ((beats - first) / pass).floor() * pass
    }
}

fn to_beats(seconds: f64, bpm: f32) -> f64 {
    seconds * bpm as f64 / 60.
}

fn to_seconds(beats: f64, bpm: f32) -> f64 {
    beats * 60. / bpm as f64
}

impl Debug for AudioClip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioClip")
            .field("id", &self.id)
            .field("audio", &self.audio.name)
            .field("position", &self.position)
            .field("length", &self.length)
            .field("offset", &self.offset)
            .field("looped", &self.looped)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pitch splits one way only, half way up being the semitone above.
    #[test]
    fn pitches_split_into_semitones_and_cents() {
        assert_eq!(split_pitch(0.49), (0, 49.));
        assert_eq!(split_pitch(0.5), (1, -50.));
        assert_eq!(split_pitch(-0.5), (0, -50.));
        assert_eq!(split_pitch(-2.25), (-2, -25.));
    }

    /// Cents past the detune's range carry over into semitones, a cent at a
    /// time, as far as the transposition goes.
    #[test]
    fn cents_carry_over_into_semitones() {
        assert_eq!(carried(0, 49.), (0, 49.));
        assert_eq!(carried(0, 50.), (1, -50.));
        assert_eq!(carried(1, -51.), (0, 49.));
        assert_eq!(carried(3, 120.), (4, 20.));
        assert_eq!(carried(24, 10.), (24, 0.));
        assert_eq!(carried(-24, -60.), (-24, 0.));
    }
    use tonique_engine::{edit::WarpMode, nodes::FadeShape};

    /// 120 bpm: two beats a second.
    const BPM: f32 = 120.;

    /// A 4 s (8-beat) file at beat 10, playing from 1 s for 4 beats.
    fn clip() -> AudioClip {
        let mut clip = AudioClip::new(ClipId(1), AudioInfo::silent(4.), BeatPos(10.), BPM);
        clip.offset = 1.;
        clip.length = 4.;
        clip
    }

    /// The same, looping 2 s to 3 s: an intro of 1 s, then the loop.
    fn looped() -> AudioClip {
        let mut clip = clip();
        clip.looped = Some(ClipLoop {
            start_s: 2.,
            end_s: 3.,
        });
        clip.length = 12.;
        clip
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_new_clip_plays_the_whole_file() {
        let clip = AudioClip::new(ClipId(1), AudioInfo::silent(4.), BeatPos(2.), BPM);
        assert_eq!((clip.offset, clip.length), (0., 8.));
        assert_eq!(clip.end(BPM), BeatPos(10.));
    }

    #[test]
    fn trimming_keeps_the_sound_in_place_within_the_file() {
        let mut c = clip();
        c.trim_start_at(BeatPos(11.), BPM);
        assert_eq!((c.position, c.offset, c.length), (BeatPos(11.), 1.5, 3.));
        // Back to the file's start, no further.
        c.trim_start_at(BeatPos(0.), BPM);
        assert_eq!((c.position, c.offset, c.length), (BeatPos(8.), 0., 6.));
        // The end, no further than the file's.
        c.trim_end_at(BeatPos(30.), BPM);
        assert_eq!(c.end(BPM), BeatPos(16.));
    }

    #[test]
    fn stretching_plays_the_same_audio_over_the_new_length() {
        // Twice as long: the same 2 s of the file, at half the speed, as if
        // its tempo were twice the project's.
        let mut c = clip();
        c.warp = WarpMode::Off;
        c.stretch_end_at(BeatPos(18.), BPM);
        assert_eq!(c.warp, WarpMode::Spectral, "warped");
        assert!(close(c.bpm, 240.) && close(c.length, 8.));
        assert_eq!((c.position, c.offset), (BeatPos(10.), 1.));
        assert!(close(c.file_seconds(c.length, BPM), 2.));
        // From the start, the end stays.
        c.stretch_start_at(BeatPos(16.), BPM);
        assert!(close(c.bpm, 60.) && close(c.length, 2.));
        assert_eq!(c.end(BPM), BeatPos(18.));
    }

    #[test]
    fn stretching_stops_at_the_stretchers_speeds() {
        let mut c = clip();
        c.stretch_end_at(BeatPos(1000.), BPM);
        assert!(close(c.bpm, BPM as f64 * 4.), "a quarter of the speed");
        assert!(close(c.length, 16.));
        c.stretch_start_at(BeatPos(25.9), BPM);
        assert!(close(c.bpm, BPM as f64 / 4.), "four times the speed");
        assert!(close(c.length, 1.));
        assert_eq!(c.end(BPM), BeatPos(26.));
    }

    #[test]
    fn a_loop_reads_on_from_its_start() {
        let c = looped();
        assert!(close(c.file_at(0., BPM), 1.), "the intro");
        assert!(close(c.file_at(2., BPM), 2.), "the loop's start");
        assert!(close(c.file_at(3.9, BPM), 2.95));
        assert!(close(c.file_at(4., BPM), 2.), "round again");
        assert!(close(c.file_at(9., BPM), 2.5));
    }

    #[test]
    fn looped_clips_grow_past_the_file_and_trim_through_the_loop() {
        let mut c = looped();
        c.trim_end_at(BeatPos(100.), BPM);
        assert_eq!(c.end(BPM), BeatPos(100.), "no file end with a loop");
        // Trimming the start into the loop reads on from there...
        c.trim_start_at(BeatPos(15.), BPM);
        assert!(close(c.offset, 2.5));
        // ...and extending it back repeats the loop, not the intro.
        c.trim_start_at(BeatPos(12.), BPM);
        assert!(close(c.offset, 2.), "{}", c.offset);
        c.trim_start_at(BeatPos(10.5), BPM);
        assert!(close(c.offset, 2.25), "{}", c.offset);
    }

    #[test]
    fn segments_cut_the_clip_at_each_pass_of_its_loop() {
        let segments = looped().segments(0.0..f64::INFINITY, BPM);
        let spans: Vec<_> = segments
            .iter()
            .map(|s| (s.start, s.end, s.file_start, s.file_end))
            .collect();
        assert_eq!(
            spans,
            [
                (0., 4., 1., 3.),
                (4., 6., 2., 3.),
                (6., 8., 2., 3.),
                (8., 10., 2., 3.),
                (10., 12., 2., 3.),
            ]
        );
        assert_eq!(
            clip().segments(0.0..f64::INFINITY, BPM).len(),
            1,
            "without a loop: one"
        );
    }

    #[test]
    fn segments_start_at_the_pass_in_range_however_far_in() {
        let segments = looped().segments(7.5..9., BPM);
        let starts: Vec<_> = segments.iter().map(|s| s.start).collect();
        assert_eq!(starts, [6., 8.], "the passes over 7.5 to 9");
        assert_eq!(looped().segments(1.0..3., BPM)[0].start, 0., "the intro");

        // A thousand passes in: still there, reading the loop.
        let mut c = looped();
        c.length = 10_000.;
        let far = c.segments(9_000.5..9_001., BPM);
        assert_eq!(far.len(), 1);
        assert_eq!(far[0].start, 9_000.);
        assert!(close(far[0].file_start, 2.) && close(far[0].file_end, 3.));
    }

    #[test]
    fn turning_a_loop_off_cuts_the_clip_at_the_files_end() {
        let mut c = looped();
        c.set_loop(None, BPM);
        assert_eq!(c.length, 6., "1 s in, 3 s of file left");
    }

    #[test]
    fn turning_a_loop_off_at_the_files_end_leaves_a_clip() {
        let mut c = looped();
        c.looped = Some(ClipLoop {
            start_s: 2.,
            end_s: 4.,
        });
        c.slip_to(4., BPM);
        c.set_loop(None, BPM);
        assert!(close(c.length, MIN_LENGTH), "{}", c.length);
        assert!(close(c.offset, 4. - to_seconds(MIN_LENGTH, BPM)));
    }

    #[test]
    fn trims_keep_the_shortest_clip() {
        let mut c = clip();
        c.trim_end_at(BeatPos(0.), BPM);
        assert_eq!(c.length, MIN_LENGTH, "the end past the start");
        let mut c = clip();
        c.trim_start_at(BeatPos(100.), BPM);
        assert_eq!(c.end(BPM), BeatPos(14.), "the end stays");
        assert!(close(c.length, MIN_LENGTH), "the start past the end");
    }

    /// Warped, a clip plays at the project's tempo over its file's;
    /// keeping what it plays of the file, it gets shorter or longer.
    #[test]
    fn warped_clips_play_at_the_project_tempo_over_theirs() {
        let mut c = clip(); // from 1 s, 4 beats: 1 s to 3 s
        assert_eq!(c.speed(BPM), 1., "new: at the project's tempo");
        c.set_warp(WarpMode::Spectral, 60., 0, 0., BPM);
        assert_eq!(c.speed(BPM), 2., "a 60 bpm file at 120");
        assert_eq!(c.length, 2., "the same 2 s of file, in half the beats");
        assert!(close(c.file_at(1., BPM), 2.), "a beat is a second of file");
        // Transposed, the pitch alone moves.
        c.set_warp(WarpMode::Spectral, 60., 7, 0., BPM);
        assert_eq!((c.speed(BPM), c.length), (2., 2.));
    }

    /// Unwarped, transposing resamples: an octave up, twice as fast.
    #[test]
    fn unwarped_clips_play_faster_transposed_up() {
        let mut c = clip();
        c.set_warp(WarpMode::Off, BPM as f64, 12, 0., BPM);
        assert!(close(c.speed(BPM), 2.));
        assert!(close(c.length, 2.));
        assert!(close(c.speed(60.), 2.), "whatever the tempo");
        // Detuned by cents.
        c.set_warp(WarpMode::Off, BPM as f64, 0, 40., BPM);
        assert!(close(c.speed(BPM), 2f64.powf(0.4 / 12.)));
        c.set_warp(WarpMode::Off, BPM as f64, 0, 80., BPM);
        assert_eq!(c.detune, MAX_DETUNE, "short of half a semitone");
    }

    #[test]
    fn slipping_stays_in_the_file() {
        let mut c = clip(); // 2 s long
        c.slip_to(0.5, BPM);
        assert_eq!(c.offset, 0.5);
        c.slip_to(10., BPM);
        assert_eq!(c.offset, 2., "still playing to the file's end");
        let mut l = looped();
        l.slip_to(10., BPM);
        assert_eq!(l.offset, 3., "up to the loop's end");
    }

    #[test]
    fn crops_keep_fades_only_at_the_ends_that_stay() {
        let faded = || {
            let mut c = clip(); // beats 10 to 14
            c.fades.fade_in.length_s = 0.25;
            c.fades.fade_out.length_s = 0.5;
            c
        };
        let fades = |c: &AudioClip| (c.fades.fade_in.length_s, c.fades.fade_out.length_s);
        let mut c = faded();
        c.crop(BeatPos(0.), BeatPos(20.), BPM);
        assert_eq!(fades(&c), (0.25, 0.5), "all of it");
        let mut c = faded();
        c.crop(BeatPos(11.), BeatPos(20.), BPM);
        assert_eq!(fades(&c), (0., 0.5), "its start cut");
        let mut c = faded();
        c.crop(BeatPos(10.), BeatPos(12.), BPM);
        assert_eq!(fades(&c), (0.25, 0.), "its end cut");
    }

    /// One fade stops at the other; a clip too short for both shows them
    /// fitted, and setting one keeps the other where it shows.
    #[test]
    fn fades_stop_at_each_other() {
        let mut c = clip(); // 2 s
        c.set_fade_in(1.5, Crossfades::default(), BPM);
        c.set_fade_out(1., Crossfades::default(), BPM);
        assert_eq!(
            (c.fades.fade_in.length_s, c.fades.fade_out.length_s),
            (1.5, 0.5)
        );
        c.set_fade_in(5., Crossfades::default(), BPM);
        assert_eq!(c.fades.fade_in.length_s, 1.5);
        c.set_fade_in(-1., Crossfades::default(), BPM);
        assert_eq!(c.fades.fade_in.length_s, 0.);

        c.set_fade_in(1.5, Crossfades::default(), BPM);
        c.trim_end_at(BeatPos(11.), BPM); // half a second
        let (i, o) = c.heard_fades(Crossfades::default(), BPM);
        assert_eq!((i.length_s, o.length_s), (0.375, 0.125));
        c.set_fade_in(0.25, Crossfades::default(), BPM);
        assert_eq!(
            (c.fades.fade_in.length_s, c.fades.fade_out.length_s),
            (0.25, 0.125)
        );
    }

    #[test]
    fn touching_clips_have_a_seam_with_what_is_trimmed_off_them() {
        // Beats 10 to 14, 1 s into a 4 s file: 1 s before, 1 s after it.
        let mut early = clip();
        early.id = ClipId(1);
        let mut late = clip();
        late.id = ClipId(2);
        late.position = BeatPos(14.);
        let curve = FadeCurve::of(FadeShape::Linear).with_curvature(0.5);
        late.fades.crossfade = Fade {
            length_s: 1.,
            curve,
        };
        let found = clip_seams(&[late.clone(), early.clone()], BPM);
        assert_eq!(found.len(), 1);
        let seam = found[0];
        assert_eq!((seam.earlier, seam.later), (ClipId(1), ClipId(2)));
        assert_eq!(seam.at, BeatPos(14.));
        assert_eq!((seam.room.before_s, seam.room.after_s), (1., 1.));
        assert_eq!((seam.before_s, seam.after_s), (0.5, 0.5));
        let ends = crossfades(&found);
        assert_eq!(ends[&ClipId(1)].end.unwrap().curve, curve.crossing());
        assert_eq!(ends[&ClipId(2)].start.unwrap().curve, curve);
        assert!(ends[&ClipId(2)].start_seam && ends[&ClipId(1)].end_seam);
        assert!(!ends[&ClipId(1)].start_seam);

        // Without a crossfade: the seam, nothing across it.
        late.fades.crossfade.length_s = 0.;
        let ends = crossfades(&clip_seams(&[late.clone(), early.clone()], BPM));
        assert!(ends[&ClipId(2)].start_seam && ends[&ClipId(2)].start.is_none());

        // Apart: none.
        late.position = BeatPos(15.);
        assert!(clip_seams(&[late, early], BPM).is_empty());
    }

    #[test]
    fn copies_have_no_crossfade() {
        let mut c = clip();
        c.fades.crossfade.length_s = 1.;
        assert_eq!(c.with_id(ClipId(9)).fades.crossfade.length_s, 0.);
        let mut kept = c.clone();
        kept.crop(BeatPos(0.), BeatPos(12.), BPM);
        assert_eq!(kept.fades.crossfade.length_s, 1., "its start stays");
        c.crop(BeatPos(11.), BeatPos(20.), BPM);
        assert_eq!(c.fades.crossfade.length_s, 0., "its start cut");
    }
}
