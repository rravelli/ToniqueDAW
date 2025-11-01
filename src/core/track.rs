use std::collections::HashMap;

use crate::{
    core::{clip::ClipCore, message::GuiToPlayerMsg},
    ui::{
        effect::UIEffect,
        effects::{EffectId, create_effect_from_id},
    },
};
use egui::Color32;
use rand::Rng;
use rtrb::Producer;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum TrackSoloState {
    Soloing,
    NotSoloing,
    Solo,
}

pub const DEFAULT_TRACK_HEIGHT: f32 = 60.;
pub const TRACK_CLOSED_HEIGHT: f32 = 22.;
/// A track containing multiple clips
#[derive(Clone, Debug)]
pub struct TrackCore {
    pub id: String,
    pub clips: Vec<ClipCore>,
    pub muted: bool,
    pub volume: f32,
    pub arm: bool,
    /// TODO Should not mix ui in the state
    effects: Vec<UIEffect>,
    pub mutable: MutableTrackCore,
    pub old_mutable: MutableTrackCore,
}

impl TrackCore {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            clips: vec![],
            muted: false,
            volume: 1.,
            arm: false,
            mutable: MutableTrackCore::new(),
            old_mutable: MutableTrackCore::new(),
            effects: vec![],
        }
    }
    pub fn from(id: &str, name: &str) -> Self {
        let mut track = Self::new();
        track.mutable.name = name.into();
        track.id = id.into();
        track
    }

    pub fn get_reference(
        &self,
        index: usize,
        selected: bool,
        solo: TrackSoloState,
    ) -> TrackReferenceCore {
        TrackReferenceCore {
            arm: self.arm,
            clips: self.clips.clone(),
            closed: self.mutable.closed,
            color: self.mutable.color,
            height: self.mutable.height,
            id: self.id.clone(),
            muted: self.muted,
            name: self.mutable.name.clone(),
            selected,
            solo,
            volume: self.volume,
            index,
        }
    }
    /// Get a mutable reference to the fields that can be changed from the UI
    pub fn get_mutable_fields(&mut self) -> &mut MutableTrackCore {
        &mut self.mutable
    }
    /// Add clips in the track without any overlap check.
    /// Do not use if you are not sure of whether the clip overlap with other clips.
    pub fn add_clips_skip_overlap_check(
        &mut self,
        clips: Vec<ClipCore>,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) {
        let mut map = HashMap::new();
        self.clips.extend(clips.clone());
        map.insert(self.id.clone(), clips);
        let _ = tx.push(GuiToPlayerMsg::AddClips(map));
    }

    /// Add clips to this track by making sure no overlap occurs.
    /// Updates at the same time the audio thread.
    /// Returns created and deleted clips in the process.
    pub fn add_clips(
        &mut self,
        added_clips: &Vec<ClipCore>,
        bpm: f32,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) -> (Vec<ClipCore>, Vec<ClipCore>) {
        let mut deleted_clips = Vec::new();
        let mut created_clips = Vec::new();

        // Fix overlap
        for added_clip in added_clips.iter() {
            let mut new_clips = vec![];
            let start = added_clip.position;
            let end = added_clip.end(bpm);

            for clip in self.clips.iter() {
                // No overlap
                if clip.position >= end || clip.end(bpm) <= start {
                    new_clips.push(clip.clone());
                    continue;
                }
                deleted_clips.push(clip.clone());
                // Sample overlaps before new clip
                if clip.position < start {
                    let mut trimmed = clip.clone_with_new_id();
                    trimmed.trim_end_at(start, bpm);

                    created_clips.push(trimmed.clone());
                    new_clips.push(trimmed);
                }

                // Sample overlaps after new clip
                if clip.end(bpm) > end {
                    let mut trimmed = clip.clone_with_new_id();
                    trimmed.trim_start_at(end, bpm);

                    created_clips.push(trimmed.clone());
                    new_clips.push(trimmed);
                }
            }
            self.clips = new_clips;
        }
        created_clips.extend(added_clips.clone());
        let mut created_map = HashMap::new();
        created_map.insert(self.id.clone(), created_clips.clone());
        self.clips.extend(added_clips.clone());

        let _ = tx.push(GuiToPlayerMsg::AddClips(created_map));
        if deleted_clips.len() > 0 {
            let _ = tx.push(GuiToPlayerMsg::RemoveClip(
                deleted_clips.iter().map(|c| c.id.clone()).collect(),
            ));
        }

        (created_clips, deleted_clips)
    }

    pub fn delete_clips(&mut self, ids: &Vec<String>, tx: &mut Producer<GuiToPlayerMsg>) {
        self.clips.retain(|clip| !ids.contains(&clip.id));
        let _ = tx.push(GuiToPlayerMsg::RemoveClip(ids.clone()));
    }

    /// Fix overlaps so that no clips overlaps **added_clip**    
    pub fn _fix_overlaps(
        &mut self,
        added_clip: &ClipCore,
        bpm: f32,
        deleted_clips: &mut Vec<ClipCore>,
        created_clips: &mut Vec<ClipCore>,
        ignore: &Vec<String>,
    ) {
        // Vec of clips after update
        let mut new_clips = vec![];
        let start = added_clip.position;
        let end = added_clip.end(bpm);
        for clip in self.clips.iter() {
            // No overlap or clip already added
            if (clip.position > end || clip.end(bpm) < start)
                || clip.id == added_clip.id
                || ignore.contains(&clip.id)
            {
                new_clips.push(clip.clone());
                continue;
            }
            deleted_clips.push(clip.clone());
            // Sample overlaps before new clip
            if clip.position < start {
                let mut trimmed = clip.clone_with_new_id();
                trimmed.trim_end_at(start, bpm);

                created_clips.push(trimmed.clone());
                new_clips.push(trimmed);
            }

            // Sample overlaps after new clip
            if clip.end(bpm) > end {
                let mut trimmed = clip.clone_with_new_id();
                trimmed.trim_start_at(end, bpm);

                created_clips.push(trimmed.clone());
                new_clips.push(trimmed);
            }
        }
        self.clips = new_clips;
    }

    pub fn cut_clip_at(
        &mut self,
        position: f32,
        bpm: f32,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) -> Option<(ClipCore, ClipCore, ClipCore)> {
        let mut found_clip = None;
        // Find corresponding clip
        for clip in self.clips.iter_mut() {
            if clip.position < position && position < clip.end(bpm) {
                let original = clip.clone();
                // Create right clip
                let mut right_clip = clip.clone_with_new_id();
                right_clip.trim_start_at(position, bpm);
                // Resize left clip
                clip.trim_end_at(position, bpm);
                found_clip = Some((original, clip.clone(), right_clip.clone()));
                break;
            }
        }
        if let Some((original, left_clip, right_clip)) = found_clip {
            // Uppdate audio thread
            let _ = tx.push(GuiToPlayerMsg::AddClip(
                self.id.clone(),
                right_clip.audio.path.clone(),
                right_clip.position,
                right_clip.id.clone(),
                right_clip.trim_start,
                right_clip.trim_end,
            ));
            let _ = tx.push(GuiToPlayerMsg::ResizeClip(
                left_clip.id.clone(),
                left_clip.trim_start,
                left_clip.trim_end,
                left_clip.position,
            ));
            // Add new clip
            self.clips.push(right_clip.clone());
            return Some((original, left_clip, right_clip));
        }
        None
    }

    pub fn duplicate_clips(
        &mut self,
        ids: &Vec<String>,
        bounds: Option<(f32, f32)>,
        bpm: f32,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) -> (Vec<ClipCore>, Vec<ClipCore>) {
        let mut created_clips = Vec::new();
        let mut deleted_clips = Vec::new();
        for id in ids {
            let clip = self.clips.iter().find(|c| c.id == *id);
            if let Some(clip) = clip {
                let mut duplicated_clip = clip.clone_with_new_id();

                if let Some((start_pos, end_pos)) = bounds {
                    duplicated_clip.trim_start_at(start_pos.max(duplicated_clip.position), bpm);
                    duplicated_clip.trim_end_at(end_pos, bpm);
                    duplicated_clip.position += end_pos - start_pos;
                } else {
                    duplicated_clip.position = clip.end(bpm);
                }
                created_clips.push(duplicated_clip);
            }
        }
        // Update track with new clips
        if !created_clips.is_empty() {
            let (created, deleted) = self.add_clips(&created_clips, bpm, tx);
            created_clips = created;
            deleted_clips.extend(deleted);
        }

        (created_clips, deleted_clips)
    }

    pub fn duplicate(&self) -> (Self, HashMap<String, String>) {
        let mut clone = self.clone();
        clone.id = Uuid::new_v4().into();

        let mut map = HashMap::new();

        for clip in &mut clone.clips {
            let old_id = clip.id.clone();
            let new_id: String = Uuid::new_v4().into();
            clip.id = new_id.clone();
            map.insert(old_id, new_id);
        }

        (clone, map)
    }

    pub fn resize_clip_skip_overlap_check(
        &mut self,
        id: &String,
        trim_start: f32,
        trim_end: f32,
        position: f32,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) {
        let Some(clip) = self.clips.iter_mut().find(|c| &c.id == id) else {
            return;
        };
        clip.trim_start = trim_start;
        clip.trim_end = trim_end;
        clip.position = position;

        let _ = tx.push(GuiToPlayerMsg::ResizeClip(
            id.clone(),
            trim_start,
            trim_end,
            position,
        ));
    }

    // Effect management
    pub fn add_effect(&mut self, id: EffectId, index: usize, tx: &mut Producer<GuiToPlayerMsg>) {
        let effect = create_effect_from_id(id);
        let _ = tx.push(GuiToPlayerMsg::AddNode(
            self.id.clone(),
            index,
            effect.id(),
            effect.get_unit(),
        ));

        self.effects
            .insert(index, UIEffect::new(effect, self.id.clone()));
    }

    pub fn remove_effects(&mut self, indexes: &Vec<usize>, tx: &mut Producer<GuiToPlayerMsg>) {
        let mut new_effects = Vec::new();
        for (i, effect) in self.effects.iter().enumerate() {
            if !indexes.contains(&i) {
                new_effects.push(effect.clone());
            } else {
                let _ = tx.push(GuiToPlayerMsg::RemoveNode(self.id.clone(), effect.id()));
            }
        }
        self.effects = new_effects;
    }

    pub fn effects_mut(&mut self) -> &mut [UIEffect] {
        &mut self.effects
    }
}

#[derive(Debug, Clone)]
pub struct TrackReferenceCore {
    pub id: String,
    pub clips: Vec<ClipCore>,
    pub muted: bool,
    pub volume: f32,
    pub arm: bool,
    pub name: String,
    pub height: f32,
    pub closed: bool,
    pub color: Color32,
    pub selected: bool,
    pub solo: TrackSoloState,
    pub index: usize,
}

impl TrackReferenceCore {
    pub fn disabled(&self) -> bool {
        self.muted && !matches!(self.solo, crate::core::track::TrackSoloState::Solo)
            || matches!(self.solo, crate::core::track::TrackSoloState::Soloing)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MutableTrackCore {
    pub name: String,
    pub height: f32,
    pub closed: bool,
    pub color: Color32,
}

impl MutableTrackCore {
    pub fn new() -> Self {
        let mut rng = rand::rng();
        let color = Color32::from_rgb(
            rng.random_range(0..=255),
            rng.random_range(0..=255),
            rng.random_range(0..=255),
        );
        Self {
            closed: false,
            height: 60.,
            color,
            name: "# Audio Track".into(),
        }
    }
}
