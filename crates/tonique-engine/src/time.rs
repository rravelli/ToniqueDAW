//! Timeline positions and the tempo map (musical time <-> samples).

/// Absolute position on the project timeline, in samples at the engine rate.
pub type SamplePos = i64;

/// Position in quarter-note beats from the start of the edit.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct BeatPos(pub f64);

impl BeatPos {
    pub const ZERO: Self = Self(0.0);

    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self(self.0.clamp(min.0, max.0))
    }

    pub fn total_cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

/// Moving a position by a length in beats.
impl std::ops::Add<f64> for BeatPos {
    type Output = Self;
    fn add(self, beats: f64) -> Self {
        Self(self.0 + beats)
    }
}

impl std::ops::Sub<f64> for BeatPos {
    type Output = Self;
    fn sub(self, beats: f64) -> Self {
        Self(self.0 - beats)
    }
}

impl std::ops::AddAssign<f64> for BeatPos {
    fn add_assign(&mut self, beats: f64) {
        self.0 += beats;
    }
}

impl std::ops::SubAssign<f64> for BeatPos {
    fn sub_assign(&mut self, beats: f64) {
        self.0 -= beats;
    }
}

/// The length in beats between two positions.
impl std::ops::Sub for BeatPos {
    type Output = f64;
    fn sub(self, other: Self) -> f64 {
        self.0 - other.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeSignature {
    pub numerator: u32,
    pub denominator: u32,
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self {
            numerator: 4,
            denominator: 4,
        }
    }
}

impl TimeSignature {
    /// Most beats a bar can count.
    pub const MAX_NUMERATOR: u32 = 32;
    /// Note values a signature can count in (half notes to sixteenths).
    pub const DENOMINATORS: [u32; 4] = [2, 4, 8, 16];

    /// `None` unless `numerator` is 1 to [`Self::MAX_NUMERATOR`] and
    /// `denominator` one of [`Self::DENOMINATORS`].
    pub fn new(numerator: u32, denominator: u32) -> Option<Self> {
        ((1..=Self::MAX_NUMERATOR).contains(&numerator)
            && Self::DENOMINATORS.contains(&denominator))
        .then_some(Self {
            numerator,
            denominator,
        })
    }

    /// Length of one bar in quarter-note beats.
    pub fn beats_per_bar(&self) -> f64 {
        self.numerator as f64 * self.beat_length()
    }

    /// Length of the beat the signature counts (its denominator's note) in
    /// quarter-note beats: 1 in 3/4, 0.5 in 6/8.
    pub fn beat_length(&self) -> f64 {
        4.0 / self.denominator as f64
    }

    /// Start of `bar` (zero-based).
    pub fn bar_start(&self, bar: i64) -> BeatPos {
        BeatPos(bar as f64 * self.beats_per_bar())
    }

    /// Where `beat` falls: its bar and counted beat (both zero-based), and
    /// how far into that beat it is, in quarter-note beats.
    pub fn locate(&self, beat: BeatPos) -> BarPosition {
        let bar = (beat.0 / self.beats_per_bar()).floor();
        let in_bar = beat.0 - bar * self.beats_per_bar();
        let counted = (in_bar / self.beat_length()).floor();
        BarPosition {
            bar: bar as i64,
            beat: counted as u32,
            offset: in_bar - counted * self.beat_length(),
        }
    }
}

impl std::fmt::Display for TimeSignature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.numerator, self.denominator)
    }
}

/// A position in bars, see [`TimeSignature::locate`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarPosition {
    pub bar: i64,
    pub beat: u32,
    pub offset: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TempoSegment {
    beat: f64,
    bpm: f64,
    /// Seconds elapsed at `beat`, cached.
    seconds: f64,
}

/// Piecewise-constant tempo map. Converting through this (instead of a
/// fixed samples-per-beat) is what keeps clips and automation glued to
/// musical time across tempo changes.
#[derive(Clone, Debug, PartialEq)]
pub struct TempoMap {
    segments: Vec<TempoSegment>,
    pub time_signature: TimeSignature,
}

impl TempoMap {
    pub fn new(bpm: f64) -> Self {
        assert!(bpm > 0.0);
        Self {
            segments: vec![TempoSegment {
                beat: 0.0,
                bpm,
                seconds: 0.0,
            }],
            time_signature: TimeSignature::default(),
        }
    }

    /// Set the tempo from `beat` onwards (replacing any change at the same beat).
    pub fn set_tempo(&mut self, beat: BeatPos, bpm: f64) {
        assert!(bpm > 0.0 && beat.0 >= 0.0);
        match self.segments.iter().position(|s| s.beat >= beat.0) {
            Some(i) if self.segments[i].beat == beat.0 => self.segments[i].bpm = bpm,
            Some(i) => self.segments.insert(
                i,
                TempoSegment {
                    beat: beat.0,
                    bpm,
                    seconds: 0.0,
                },
            ),
            None => self.segments.push(TempoSegment {
                beat: beat.0,
                bpm,
                seconds: 0.0,
            }),
        }
        self.recompute();
    }

    /// Remove a tempo change (the one at beat 0 can't be removed).
    pub fn remove_tempo(&mut self, beat: BeatPos) {
        if beat.0 > 0.0 {
            self.segments.retain(|s| s.beat != beat.0);
            self.recompute();
        }
    }

    pub fn tempo_changes(&self) -> impl Iterator<Item = (BeatPos, f64)> + '_ {
        self.segments.iter().map(|s| (BeatPos(s.beat), s.bpm))
    }

    fn recompute(&mut self) {
        for i in 1..self.segments.len() {
            let p = self.segments[i - 1];
            self.segments[i].seconds = p.seconds + (self.segments[i].beat - p.beat) * 60.0 / p.bpm;
        }
    }

    pub fn bpm_at(&self, beat: BeatPos) -> f64 {
        self.segment_for_beat(beat.0).bpm
    }

    fn segment_for_beat(&self, beat: f64) -> &TempoSegment {
        let i = self.segments.partition_point(|s| s.beat <= beat);
        &self.segments[i.saturating_sub(1)]
    }

    pub fn beats_to_seconds(&self, beat: BeatPos) -> f64 {
        let s = self.segment_for_beat(beat.0);
        s.seconds + (beat.0 - s.beat) * 60.0 / s.bpm
    }

    pub fn seconds_to_beats(&self, seconds: f64) -> BeatPos {
        let i = self.segments.partition_point(|s| s.seconds <= seconds);
        let s = &self.segments[i.saturating_sub(1)];
        BeatPos(s.beat + (seconds - s.seconds) * s.bpm / 60.0)
    }

    pub fn beats_to_samples(&self, beat: BeatPos, sample_rate: f64) -> SamplePos {
        (self.beats_to_seconds(beat) * sample_rate).round() as SamplePos
    }

    pub fn samples_to_beats(&self, pos: SamplePos, sample_rate: f64) -> BeatPos {
        self.seconds_to_beats(pos as f64 / sample_rate)
    }

    /// `bar` and `beat` are zero-based.
    pub fn bar_beat_to_beats(&self, bar: u32, beat: f64) -> BeatPos {
        self.time_signature.bar_start(bar as i64) + beat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_tempo() {
        let t = TempoMap::new(120.0);
        assert_eq!(t.beats_to_seconds(BeatPos(4.0)), 2.0);
        assert_eq!(t.beats_to_samples(BeatPos(1.0), 48000.0), 24000);
        assert_eq!(t.seconds_to_beats(1.5), BeatPos(3.0));
    }

    #[test]
    fn tempo_change_roundtrips() {
        let mut t = TempoMap::new(120.0);
        t.set_tempo(BeatPos(4.0), 60.0);
        // 4 beats @120 = 2s, then 2 beats @60 = 2s.
        assert!((t.beats_to_seconds(BeatPos(6.0)) - 4.0).abs() < 1e-12);
        for b in [0.0, 1.3, 4.0, 7.77] {
            let back = t.seconds_to_beats(t.beats_to_seconds(BeatPos(b)));
            assert!((back.0 - b).abs() < 1e-9);
        }
        assert_eq!(t.bpm_at(BeatPos(3.99)), 120.0);
        assert_eq!(t.bpm_at(BeatPos(4.0)), 60.0);
        t.remove_tempo(BeatPos(4.0));
        assert_eq!(t.beats_to_seconds(BeatPos(6.0)), 3.0);
    }

    #[test]
    fn signatures_are_validated() {
        assert_eq!(TimeSignature::new(4, 4), Some(TimeSignature::default()));
        assert!(TimeSignature::new(7, 8).is_some());
        assert!(TimeSignature::new(0, 4).is_none());
        assert!(TimeSignature::new(33, 4).is_none());
        assert!(TimeSignature::new(4, 3).is_none());
        assert_eq!(TimeSignature::new(6, 8).unwrap().to_string(), "6/8");
    }

    /// Bars and counted beats, in quarter-note beats.
    #[test]
    fn signatures_measure_bars() {
        let sig = |n, d| TimeSignature::new(n, d).unwrap();
        assert_eq!(sig(3, 4).beats_per_bar(), 3.0);
        assert_eq!(sig(6, 8).beats_per_bar(), 3.0);
        assert_eq!(sig(6, 8).beat_length(), 0.5);
        assert_eq!(sig(7, 8).bar_start(2), BeatPos(7.0));
        assert_eq!(sig(2, 2).beats_per_bar(), 4.0);

        // 7/8: bar 2 starts at 3.5; 5.25 is its fourth eighth, plus 0.25.
        let at = sig(7, 8).locate(BeatPos(5.25));
        assert_eq!((at.bar, at.beat, at.offset), (1, 3, 0.25));
        let at = sig(3, 4).locate(BeatPos(3.0));
        assert_eq!((at.bar, at.beat, at.offset), (1, 0, 0.0));
    }
}
