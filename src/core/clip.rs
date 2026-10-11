use crate::analysis::AudioInfo;
use std::fmt::Debug;
use tonique_engine::{
    edit::{ClipId, ClipLoop},
    time::BeatPos,
};

/// Most pieces [`AudioClip::segments`] cuts a clip into: a tiny loop over
/// a long clip would make too many to draw.
const MAX_SEGMENTS: usize = 4096;

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
        };
        clip.length = clip.source_beats(bpm);
        clip
    }

    pub fn with_id(&self, id: ClipId) -> Self {
        let mut clone = self.clone();
        clone.id = id;
        clone
    }

    /// Length of the whole source file, in seconds.
    pub fn source_seconds(&self) -> f32 {
        self.audio.duration.map_or(0., |d| d.as_secs_f32())
    }

    /// Length of the whole source file, in beats.
    pub fn source_beats(&self, bpm: f32) -> f64 {
        to_beats(self.source_seconds() as f64, bpm)
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
        let at = self.offset + to_seconds(beats, bpm);
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
            to_beats(self.source_seconds() as f64 - self.offset, bpm).max(0.)
        }
    }

    /// Lengths are kept in beats: `_bpm` only for symmetry with MIDI clips.
    pub fn end(&self, _bpm: f32) -> BeatPos {
        self.position + self.length
    }

    /// Where the start of the file lies in the arrangement, read without
    /// looping: before the clip's position by its offset.
    pub fn file_start(&self, bpm: f32) -> BeatPos {
        self.position - to_beats(self.offset, bpm)
    }

    /// Move the clip's start to `beats`, keeping its end and its sound in
    /// place: what's left of the start goes; extending it reveals what
    /// played before, no earlier than the first beat, nor (without a loop)
    /// the start of the file. A loop goes on repeating backwards. Does
    /// nothing when the file's length is unknown.
    pub fn trim_start_at(&mut self, beats: BeatPos, bpm: f32) {
        if self.source_seconds() <= 0. {
            return;
        }
        let end = self.end(bpm);
        let mut beats = beats.clamp(BeatPos::ZERO, end);
        let looping_back = self.active_loop().filter(|l| self.offset >= l.start_s);
        if beats < self.position && looping_back.is_none() {
            // Not before the file's start.
            beats = beats.max(self.file_start(bpm));
        }
        let shift = beats - self.position;
        self.offset = match looping_back {
            Some(l) if shift < 0. => {
                let back = self.offset - l.start_s + to_seconds(shift, bpm);
                l.start_s + back.rem_euclid(l.end_s - l.start_s)
            }
            _ => self.file_at(shift, bpm),
        };
        self.position = beats;
        self.length = end - beats;
    }

    /// Move the clip's end to `beats`, keeping its start: no further than
    /// the file's end, unless it loops. Does nothing when the file's length
    /// is unknown.
    pub fn trim_end_at(&mut self, beats: BeatPos, bpm: f32) {
        if self.source_seconds() <= 0. {
            return;
        }
        self.length = (beats - self.position).clamp(0., self.max_length(bpm));
    }

    /// Keep only the part of the clip inside `start..end` (in beats). Never
    /// reveals audio that was trimmed away.
    pub fn crop(&mut self, start: BeatPos, end: BeatPos, bpm: f32) {
        let clip_end = self.end(bpm);
        self.trim_start_at(start.max(self.position), bpm);
        self.trim_end_at(end.min(clip_end), bpm);
    }

    /// Start (or stop) looping `looped`. Turning a loop off cuts the clip
    /// at the file's end.
    pub fn set_loop(&mut self, looped: Option<ClipLoop>, bpm: f32) {
        self.looped = looped;
        self.length = self.length.min(self.max_length(bpm));
    }

    /// Play from `offset` seconds into the file, the clip staying where it
    /// is: within the file, and (without a loop) no further than lets it
    /// play to its end.
    pub fn slip_to(&mut self, offset: f64, bpm: f32) {
        let source = self.source_seconds() as f64;
        let last = match self.active_loop() {
            Some(l) => l.end_s,
            None => source - to_seconds(self.length, bpm),
        };
        self.offset = offset.clamp(0., last.max(0.));
    }

    /// The clip cut where it jumps, at each pass of its loop: what to draw.
    pub fn segments(&self, bpm: f32) -> Vec<Segment> {
        let mut segments = Vec::new();
        let mut start = 0.;
        while start < self.length && segments.len() < MAX_SEGMENTS {
            let file_start = self.file_at(start, bpm);
            // Until the loop's end, or the file's.
            let until = match self.active_loop() {
                Some(l) if file_start < l.end_s => l.end_s,
                _ => self.source_seconds() as f64,
            };
            let end = (start + to_beats(until - file_start, bpm)).min(self.length);
            if end <= start {
                break;
            }
            segments.push(Segment {
                start,
                end,
                file_start,
                file_end: file_start + to_seconds(end - start, bpm),
            });
            start = end;
        }
        segments
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
        let segments = looped().segments(BPM);
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
        assert_eq!(clip().segments(BPM).len(), 1, "without a loop: one");
    }

    #[test]
    fn turning_a_loop_off_cuts_the_clip_at_the_files_end() {
        let mut c = looped();
        c.set_loop(None, BPM);
        assert_eq!(c.length, 6., "1 s in, 3 s of file left");
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
}
