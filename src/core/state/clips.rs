//! Clip edits, turned into engine commands, and the clips as the UI sees
//! them.

use crate::core::{
    clip::AudioClip,
    state::clip_ops::{ClipOp, TrackClips},
    track::TrackRow,
};

use tonique_engine::{
    edit::{
        Clip, ClipContent, ClipId, Parameter, TrackId,
        commands::{
            AddClip, MoveClip, RemoveClip, ResizeClip, SetClipLoop, SetClipOrigin, SetParam,
        },
    },
    time::BeatPos,
};

use super::{ClipParam, ProjectState};

impl ProjectState {
    pub(super) fn clip_ops(
        &mut self,
        label: &'static str,
        f: impl FnOnce(&mut Self, &mut Vec<ClipOp>),
    ) {
        let mut ops = Vec::new();
        f(self, &mut ops);
        self.transaction(label, |s| {
            for op in ops {
                s.apply_clip_op(op);
            }
        });
    }

    pub(super) fn track_clips<'a>(
        &self,
        track: TrackId,
        ops: &'a mut Vec<ClipOp>,
    ) -> TrackClips<'a> {
        TrackClips {
            track,
            clips: self.clip_views(track),
            bpm: self.bpm(),
            ops,
        }
    }

    /// Add clips and fix all overlaps on the track.
    pub fn add_clips(&mut self, track_id: &TrackId, mut clips: Vec<AudioClip>) {
        // Clips never start before the first beat.
        for clip in &mut clips {
            clip.position = clip.position.max(BeatPos::ZERO);
        }
        self.clip_ops("Add clips", |s, ops| {
            let mut clips_on_track = s.track_clips(*track_id, ops);
            clips_on_track.add(clips, &mut || s.new_clip_id());
        });
    }
    /// Move clip to a new position and a new track fixing all overlaps on this track.
    /// Clips never start before the first beat.
    pub fn move_clip(
        &mut self,
        id: &ClipId,
        to_track: &TrackId,
        to_pos: BeatPos,
        ignore: &[ClipId],
    ) {
        let Some((from, clip)) = self.find_clip(*id) else {
            return;
        };
        let to_pos = to_pos.max(BeatPos::ZERO);
        self.clip_ops("Move clips", |s, ops| {
            let mut moved = clip.clone();
            moved.position = to_pos;
            let mut ignore = ignore.to_vec();
            ignore.push(*id);
            let end = moved.end(s.bpm());
            let mut clips_on_track = s.track_clips(*to_track, ops);
            clips_on_track.carve(to_pos, end, &ignore, &mut || s.new_clip_id());
            ops.push(ClipOp::Move {
                from,
                to: *to_track,
                clip: *id,
                position: to_pos,
            });
        });
    }
    /// Delete clips for their ids
    pub fn delete_clips(&mut self, ids: &[ClipId]) {
        self.clip_ops("Delete clips", |s, ops| {
            for id in ids {
                if let Some((track, _)) = s.find_clip(*id) {
                    ops.push(ClipOp::Remove(track, *id));
                }
            }
        });
    }
    /// Cut clip located at position on given track. Does nothing it there is no clip.
    pub fn cut_clip_at(&mut self, track_id: &TrackId, position: BeatPos) {
        self.clip_ops("Cut clip", |s, ops| {
            let mut clips_on_track = s.track_clips(*track_id, ops);
            clips_on_track.cut_at(position, &mut || s.new_clip_id());
        });
    }
    /// Duplicate clips fixing all overlaps on the tracks.
    pub fn duplicate_clips(
        &mut self,
        ids: &[ClipId],
        bounds: Option<(BeatPos, BeatPos)>,
    ) -> Vec<ClipId> {
        let mut copy_ids = Vec::new();
        self.clip_ops("Duplicate clips", |s, ops| {
            let tracks: Vec<_> = s.edit().tracks.iter().map(|t| t.id).collect();
            for track in tracks {
                let mut clips_on_track = s.track_clips(track, ops);
                let mut new_id = || s.new_clip_id();
                let copies = clips_on_track.duplicates(ids, bounds, &mut new_id);
                copy_ids.extend(copies.iter().map(|clip| clip.id));
                clips_on_track.add(copies, &mut new_id);
            }
        });
        copy_ids
    }
    /// Show a clip trimmed (or moved) as `resized` while dragging, without
    /// touching the edit. Use `commit_resize_clip` to apply it.
    pub fn resize_clip(&mut self, resized: AudioClip) {
        self.resized_clip = Some(resized);
    }
    /// Apply a trim as one undo step, trimming the clips it now overlaps.
    /// Nothing changes (and no step is recorded) when it's as in the edit.
    pub fn commit_resize_clip(&mut self, resized: AudioClip) {
        self.resized_clip = None;
        let Some((track, clip)) = self.find_clip(resized.id) else {
            return;
        };
        let region = |c: &AudioClip| (c.position, c.length, c.offset, c.looped);
        if region(&clip) == region(&resized) {
            return;
        }
        let label = if clip.looped != resized.looped {
            "Clip loop"
        } else {
            "Resize clip"
        };
        self.clip_ops(label, |s, ops| {
            let (start, end) = (resized.position, resized.end(s.bpm()));
            let mut clips_on_track = s.track_clips(track, ops);
            clips_on_track.carve(start, end, &[resized.id], &mut || s.new_clip_id());
            ops.push(ClipOp::Resize(track, resized));
        });
    }
    /// Move bar 1 of clip `id`'s own grid to `origin` seconds into its file,
    /// as an undo step.
    pub fn set_clip_origin(&mut self, id: ClipId, origin: f64) {
        let Some((track, clip)) = self.find_clip(id) else {
            return;
        };
        let origin = origin.clamp(0., clip.source_seconds() as f64);
        if origin != clip.origin {
            self.perform(SetClipOrigin::new(track, id, origin));
        }
    }
    /// Set an audio clip's gain (linear) or pan, heard at once. Not an
    /// undo step: see [`Self::commit_clip_param`].
    pub fn set_clip_param(&mut self, id: ClipId, which: ClipParam, value: f32) {
        if let Some(param) = self.clip_param(id, which) {
            param.set(value);
        }
    }
    /// Set an audio clip's gain or pan as an undo step from `old`, its value
    /// before the user started changing it.
    pub fn commit_clip_param(&mut self, id: ClipId, which: ClipParam, old: f32, new: f32) {
        let Some(param) = self.clip_param(id, which) else {
            return;
        };
        if old == new {
            return param.set(new);
        }
        // `SetParam` records the current value for undo: restore it first.
        param.set(old);
        let id = param.id;
        self.perform(SetParam::new(id, new));
    }
    fn clip_param(&self, id: ClipId, which: ClipParam) -> Option<Parameter> {
        let clip = self.edit().tracks.iter().find_map(|t| t.clip(id))?;
        match which {
            ClipParam::Gain => clip.gain(),
            ClipParam::Pan => clip.pan(),
        }
        .cloned()
    }
    /// Whether clip `id` is being trimmed (see [`Self::resize_clip`]).
    pub fn is_resizing(&self, id: ClipId) -> bool {
        self.resized_clip.as_ref().is_some_and(|r| r.id == id)
    }
    /// `clip` as shown: as being trimmed, if it is.
    pub fn shown_clip(&self, clip: &AudioClip) -> AudioClip {
        self.resized_clip
            .as_ref()
            .filter(|r| r.id == clip.id)
            .unwrap_or(clip)
            .clone()
    }

    fn apply_clip_op(&mut self, op: ClipOp) {
        match op {
            ClipOp::Add(track, clip) => {
                let clip = self.engine_clip(&clip);
                self.perform(AddClip::new(track, clip));
            }
            ClipOp::Remove(track, id) => self.perform(RemoveClip::new(track, id)),
            ClipOp::Resize(track, clip) => {
                let looped = self
                    .edit()
                    .track(track)
                    .ok()
                    .and_then(|t| t.clip(clip.id))
                    .and_then(|c| match &c.content {
                        ClipContent::Audio { looped, .. } => Some(*looped),
                        ClipContent::Midi { .. } => None,
                    });
                if looped.is_some_and(|l| l != clip.looped) {
                    self.perform(SetClipLoop::new(track, clip.id, clip.looped));
                }
                self.perform(ResizeClip::new(
                    track,
                    clip.id,
                    clip.position,
                    clip.length,
                    clip.offset,
                ));
            }
            ClipOp::Move {
                from,
                to,
                clip,
                position,
            } => self.perform(MoveClip::to_track(from, clip, to, position)),
        }
    }

    /// The engine clip for a UI clip, registering its audio file if needed.
    fn engine_clip(&mut self, clip: &AudioClip) -> Clip {
        let session = &mut self.session;
        let source = self
            .sources
            .get_or_insert(&clip.audio, || session.create(|e| e.new_source()));
        let mut engine_clip = self
            .session
            .create(|e| Clip::audio(e, clip.position, clip.length, source));
        engine_clip.id = clip.id;
        if let ClipContent::Audio {
            source_offset_s,
            gain,
            pan,
            looped,
            origin_s,
            ..
        } = &mut engine_clip.content
        {
            *source_offset_s = clip.offset;
            *origin_s = clip.origin;
            // In range, whatever a project file says.
            gain.value.set(clip.gain.clamp(gain.min, gain.max), 0.);
            pan.value.set(clip.pan.clamp(pan.min, pan.max), 0.);
            *looped = clip.looped;
        }
        engine_clip
    }

    /// The UI view of an engine clip. `None` for MIDI clips, which the UI
    /// can't show yet.
    pub(super) fn clip_view(&self, clip: &Clip) -> Option<AudioClip> {
        let ClipContent::Audio {
            source,
            source_offset_s,
            ref gain,
            ref pan,
            looped,
            origin_s,
        } = clip.content
        else {
            return None;
        };
        let audio = self.sources.info(source)?.clone();
        // Clips of files of unknown length can't be edited.
        audio.duration?;
        Some(AudioClip {
            id: clip.id,
            audio,
            position: clip.start,
            length: clip.length,
            offset: source_offset_s,
            looped,
            gain: gain.get(),
            pan: pan.get(),
            origin: origin_s,
        })
    }

    fn clip_views(&self, track: TrackId) -> Vec<AudioClip> {
        self.edit()
            .track(track)
            .map(|t| t.clips.iter().filter_map(|c| self.clip_view(c)).collect())
            .unwrap_or_default()
    }

    /// Clip `id` and the row of its track, if it exists.
    pub fn clip_with_track(&self, id: ClipId) -> Option<(TrackRow, AudioClip)> {
        let (track, clip) = self.find_clip(id)?;
        let track = self.track_from_index(self.track_index(track)?)?;
        Some((track, clip))
    }

    pub(crate) fn find_clip(&self, id: ClipId) -> Option<(TrackId, AudioClip)> {
        let track = self.edit().tracks.iter().find(|t| t.clip(id).is_some())?;
        let clip = self.clip_view(track.clip(id)?)?;
        Some((track.id, clip))
    }
}
