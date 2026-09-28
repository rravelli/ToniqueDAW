//! Clip editing rules (overlaps, cuts, duplicates) as pure functions over
//! snapshots of a track's clips. They return [`ClipOp`]s, which the state
//! turns into engine commands inside one undo transaction.

use crate::core::clip::AudioClip;
use tonique_engine::edit::{ClipId, TrackId};

#[derive(Debug, Clone)]
pub enum ClipOp {
    Add(TrackId, AudioClip),
    Remove(TrackId, ClipId),
    /// Change the clip's position and trims to those of the given clip.
    Resize(TrackId, AudioClip),
    Move {
        from: TrackId,
        to: TrackId,
        clip: ClipId,
        position: f32,
    },
}

/// A track's clips, edited locally while ops are recorded, so successive
/// operations see each other's effects.
pub struct TrackClips<'a> {
    pub track: TrackId,
    pub clips: Vec<AudioClip>,
    pub bpm: f32,
    pub ops: &'a mut Vec<ClipOp>,
}

impl TrackClips<'_> {
    /// Make room for `[start, end)`: clips overlapping it are trimmed, split
    /// or removed. Clips in `ignore` are left alone.
    pub fn carve(
        &mut self,
        start: f32,
        end: f32,
        ignore: &[ClipId],
        new_id: &mut impl FnMut() -> ClipId,
    ) {
        let bpm = self.bpm;
        let mut kept = Vec::new();
        for clip in std::mem::take(&mut self.clips) {
            if ignore.contains(&clip.id) || clip.position >= end || clip.end(bpm) <= start {
                kept.push(clip);
                continue;
            }
            let left = (clip.position < start).then(|| {
                let mut c = clip.clone();
                c.trim_end_at(start, bpm);
                c
            });
            let right = (clip.end(bpm) > end).then(|| {
                let mut c = clip.clone();
                c.trim_start_at(end, bpm);
                c
            });
            match (left, right) {
                (Some(l), Some(r)) => {
                    let r = r.with_id(new_id());
                    self.ops.push(ClipOp::Resize(self.track, l.clone()));
                    self.ops.push(ClipOp::Add(self.track, r.clone()));
                    kept.extend([l, r]);
                }
                (Some(piece), None) | (None, Some(piece)) => {
                    self.ops.push(ClipOp::Resize(self.track, piece.clone()));
                    kept.push(piece);
                }
                (None, None) => self.ops.push(ClipOp::Remove(self.track, clip.id)),
            }
        }
        self.clips = kept;
    }

    /// Add clips, trimming whatever they overlap.
    pub fn add(&mut self, added: Vec<AudioClip>, new_id: &mut impl FnMut() -> ClipId) {
        for clip in added {
            self.carve(clip.position, clip.end(self.bpm), &[], new_id);
            self.ops.push(ClipOp::Add(self.track, clip.clone()));
            self.clips.push(clip);
        }
    }

    /// Split the clip under `position` in two.
    pub fn cut_at(&mut self, position: f32, new_id: &mut impl FnMut() -> ClipId) {
        let bpm = self.bpm;
        let Some(clip) = self
            .clips
            .iter()
            .find(|c| c.position < position && position < c.end(bpm))
        else {
            return;
        };
        let mut left = clip.clone();
        left.trim_end_at(position, bpm);
        let mut right = clip.with_id(new_id());
        right.trim_start_at(position, bpm);
        self.ops.push(ClipOp::Resize(self.track, left));
        self.ops.push(ClipOp::Add(self.track, right));
    }

    /// Copies of the clips in `ids`: right after each clip, or when `bounds`
    /// (a time selection) is given, the selected part right after the
    /// selection. Returns the copies without adding them.
    pub fn duplicates(
        &self,
        ids: &[ClipId],
        bounds: Option<(f32, f32)>,
        new_id: &mut impl FnMut() -> ClipId,
    ) -> Vec<AudioClip> {
        let bpm = self.bpm;
        self.clips
            .iter()
            .filter(|c| ids.contains(&c.id))
            .map(|clip| {
                let mut copy = clip.with_id(new_id());
                match bounds {
                    Some((start, end)) => {
                        copy.crop(start, end, bpm);
                        copy.position += end - start;
                    }
                    None => copy.position = clip.end(bpm),
                }
                copy
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{AudioData, AudioInfo};
    use std::{path::PathBuf, sync::Arc, time::Duration};

    const BPM: f32 = 60.; // one beat per second: clip seconds == beats

    fn clip(id: u64, position: f32, seconds: f32) -> AudioClip {
        let audio = AudioInfo {
            name: "test".into(),
            duration: Some(Duration::from_secs_f32(seconds)),
            data: Arc::new(AudioData::default()),
            sample_rate: 48000,
            channels: 2,
            bit_depth: None,
            num_samples: None,
            path: PathBuf::from("test.wav"),
        };
        AudioClip::new(ClipId(id), audio, position)
    }

    fn ids() -> impl FnMut() -> ClipId {
        let mut next = 100;
        move || {
            next += 1;
            ClipId(next)
        }
    }

    fn span(c: &AudioClip) -> (f32, f32) {
        (c.position, c.end(BPM))
    }

    #[test]
    fn adding_a_clip_trims_splits_and_removes_overlaps() {
        let mut ops = Vec::new();
        let mut t = TrackClips {
            track: TrackId(1),
            clips: vec![clip(1, 0., 4.), clip(2, 5., 1.), clip(3, 8., 4.)],
            bpm: BPM,
            ops: &mut ops,
        };
        // Covers the end of 1, all of 2 and the start of 3.
        t.add(vec![clip(4, 2., 8.)], &mut ids());
        let mut spans: Vec<_> = t.clips.iter().map(|c| (c.id, span(c))).collect();
        spans.sort_by(|a, b| a.1.0.total_cmp(&b.1.0));
        assert_eq!(
            spans,
            vec![
                (ClipId(1), (0., 2.)),
                (ClipId(4), (2., 10.)),
                (ClipId(3), (10., 12.)),
            ]
        );
        assert!(matches!(ops[1], ClipOp::Remove(_, ClipId(2))));
    }

    #[test]
    fn adding_inside_a_clip_splits_it() {
        let mut ops = Vec::new();
        let mut t = TrackClips {
            track: TrackId(1),
            clips: vec![clip(1, 0., 10.)],
            bpm: BPM,
            ops: &mut ops,
        };
        t.add(vec![clip(2, 4., 2.)], &mut ids());
        let mut spans: Vec<_> = t.clips.iter().map(span).collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (got, want) in spans.iter().zip([(0., 4.), (4., 6.), (6., 10.)]) {
            assert!((got.0 - want.0).abs() < 1e-4 && (got.1 - want.1).abs() < 1e-4);
        }
        assert!(matches!(&ops[1], ClipOp::Add(_, c) if c.id == ClipId(101)));
    }

    #[test]
    fn touching_clips_are_left_alone() {
        let mut ops = Vec::new();
        let mut t = TrackClips {
            track: TrackId(1),
            clips: vec![clip(1, 0., 2.), clip(2, 4., 2.)],
            bpm: BPM,
            ops: &mut ops,
        };
        t.carve(2., 4., &[], &mut ids());
        assert!(ops.is_empty());
    }

    #[test]
    fn cut_and_duplicate() {
        let mut ops = Vec::new();
        let mut t = TrackClips {
            track: TrackId(1),
            clips: vec![clip(1, 0., 4.)],
            bpm: BPM,
            ops: &mut ops,
        };
        let copies = t.duplicates(&[ClipId(1)], None, &mut ids());
        assert_eq!(span(&copies[0]), (4., 8.));
        let copies = t.duplicates(&[ClipId(1)], Some((1., 3.)), &mut ids());
        let (a, b) = span(&copies[0]);
        assert!((a - 3.).abs() < 1e-4 && (b - 5.).abs() < 1e-4);

        t.cut_at(1., &mut ids());
        t.cut_at(4., &mut ids()); // on the edge: nothing to cut
        assert_eq!(ops.len(), 2);
        let (ClipOp::Resize(_, left), ClipOp::Add(_, right)) = (&ops[0], &ops[1]) else {
            panic!("{ops:?}")
        };
        assert_eq!(span(left), (0., 1.));
        assert_eq!(span(right), (1., 4.));
    }

    #[test]
    fn duplicating_a_zone_past_a_trimmed_clip_keeps_the_trim() {
        let mut ops = Vec::new();
        let mut trimmed = clip(1, 0., 4.);
        trimmed.trim_end_at(2., BPM); // audible 0..2 out of 0..4
        let t = TrackClips {
            track: TrackId(1),
            clips: vec![trimmed],
            bpm: BPM,
            ops: &mut ops,
        };
        let copies = t.duplicates(&[ClipId(1)], Some((1., 6.)), &mut ids());
        let (a, b) = span(&copies[0]);
        assert!((a - 6.).abs() < 1e-4 && (b - 7.).abs() < 1e-4, "{a}..{b}");
    }
}
