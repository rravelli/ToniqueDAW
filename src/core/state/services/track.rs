use crate::core::{
    clip::ClipCore,
    message::GuiToPlayerMsg,
    track::{MutableTrackCore, TrackCore, TrackReferenceCore, TrackSoloState},
};
use rtrb::Producer;
use std::collections::HashMap;

/// Service managing tracks
pub struct TrackService {
    tracks: HashMap<String, TrackCore>,
    order: Vec<String>,

    solo_tracks: Vec<String>,
    pub selected_tracks: Vec<String>,
}

impl TrackService {
    pub fn new() -> Self {
        let mut tracks = HashMap::new();
        tracks.insert("master".into(), TrackCore::from("master", "Master"));
        Self {
            tracks: tracks,
            order: Vec::new(),
            solo_tracks: Vec::new(),
            selected_tracks: Vec::new(),
        }
    }
    // Getters
    pub fn length(&self) -> usize {
        self.order.len()
    }

    pub fn get(&mut self, id: &String) -> Option<&mut TrackCore> {
        self.tracks.get_mut(id)
    }

    pub fn get_mut(&mut self, id: &String) -> &mut MutableTrackCore {
        self.tracks.get_mut(id).unwrap().get_mutable_fields()
    }

    pub fn from_index(&self, index: usize) -> Option<TrackReferenceCore> {
        if index >= self.length() {
            return None;
        }
        self.get_reference(&self.order[index])
    }

    pub fn get_reference(&self, id: &String) -> Option<TrackReferenceCore> {
        self.tracks.get(id).map(|t| {
            t.get_reference(
                self.order.iter().position(|t_id| *t_id == *id).unwrap_or(0),
                self.selected_tracks.contains(&t.id),
                if self.solo_tracks.is_empty() {
                    TrackSoloState::NotSoloing
                } else if self.solo_tracks.contains(&t.id) {
                    TrackSoloState::Solo
                } else {
                    TrackSoloState::Soloing
                },
            )
        })
    }

    pub fn tracks(&self) -> impl Iterator<Item = TrackReferenceCore> {
        // get tracks in order
        self.order
            .iter()
            .filter_map(|id| {
                if *id != "master" {
                    self.tracks.get(id)
                } else {
                    None
                }
            })
            .enumerate()
            .map(|(index, t)| {
                t.get_reference(
                    index,
                    self.selected_tracks.contains(&t.id),
                    if self.solo_tracks.is_empty() {
                        TrackSoloState::NotSoloing
                    } else if self.solo_tracks.contains(&t.id) {
                        TrackSoloState::Solo
                    } else {
                        TrackSoloState::Soloing
                    },
                )
            })
    }
    pub fn master_track(&self) -> TrackReferenceCore {
        let master_id = "master".to_string();
        self.tracks.get(&master_id).unwrap().get_reference(
            0,
            self.selected_tracks.contains(&master_id),
            TrackSoloState::NotSoloing,
        )
    }
    // Mutations
    /// Create a new track at position `index` creating the track and the clips
    pub fn insert(&mut self, track: TrackCore, index: usize, tx: &mut Producer<GuiToPlayerMsg>) {
        let _ = tx.push(GuiToPlayerMsg::AddTrack(track.id.clone()));
        if track.clips.len() > 0 {
            let mut map = HashMap::new();
            map.insert(track.id.clone(), track.clips.clone());
            let _ = tx.push(GuiToPlayerMsg::AddClips(map));
        }
        self.order.insert(index, track.id.clone());
        self.tracks.insert(track.id.clone(), track);
    }
    /// Delete a track from its `id`. Returns the deleted track and its position if the track was found.
    pub fn delete(
        &mut self,
        id: &String,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) -> Option<(TrackCore, usize)> {
        let pos = self.order.iter().position(|x| *x == *id)?;
        let track = self.tracks.get(id)?.clone();

        self.order.remove(pos);
        self.selected_tracks.retain(|sel| sel != id);
        self.solo_tracks.retain(|sel| sel != id);
        self.tracks.remove(id);

        let _ = tx.push(GuiToPlayerMsg::RemoveTrack(id.clone()));

        Some((track, pos))
    }
    /// Remove clips if their id is in given `ids`
    pub fn delete_clips(
        &mut self,
        ids: &Vec<String>,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) -> HashMap<String, Vec<ClipCore>> {
        let mut deleted_clips = HashMap::new();
        for (track_id, track) in self.tracks.iter_mut() {
            let mut removed = Vec::new();
            track.clips.retain(|clip| {
                if ids.contains(&clip.id) {
                    removed.push(clip.clone()); // ClipCore must be Clone
                    false
                } else {
                    true
                }
            });
            if !removed.is_empty() {
                deleted_clips.insert(track_id.clone(), removed);
            }
        }
        let _ = tx.push(GuiToPlayerMsg::RemoveClip(ids.clone()));
        deleted_clips
    }
    /// Duplicate multiple clips within selected bounds. All overlaps are fixed in each track. Returns the newly created clips
    pub fn duplicate_clips(
        &mut self,
        ids: &Vec<String>,
        bounds: Option<(f32, f32)>,
        bpm: f32,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) -> (
        HashMap<String, Vec<ClipCore>>,
        HashMap<String, Vec<ClipCore>>,
    ) {
        let mut created_clips = HashMap::new();
        let mut deleted_clips = HashMap::new();
        for track in self.tracks.values_mut().filter(|t| t.id != "master") {
            let (created, deleted) = track.duplicate_clips(ids, bounds, bpm, tx);
            if !created.is_empty() {
                created_clips.insert(track.id.clone(), created);
            }
            if !deleted.is_empty() {
                deleted_clips.insert(track.id.clone(), deleted);
            }
        }
        (created_clips, deleted_clips)
    }
    /// Set the gain of a given track
    pub fn set_volume(&mut self, id: &String, volume: f32, tx: &mut Producer<GuiToPlayerMsg>) {
        if let Some(track) = self.tracks.get_mut(id) {
            track.volume = volume;
            let _ = tx.push(GuiToPlayerMsg::ChangeTrackVolume(id.clone(), volume));
        }
    }
    /// Mute a track
    pub fn set_mute(&mut self, id: String, mute: bool, tx: &mut Producer<GuiToPlayerMsg>) {
        if let Some(track) = self.tracks.get_mut(&id) {
            track.muted = mute;
            let _ = tx.push(GuiToPlayerMsg::MuteTrack(id, mute));
        }
    }
    /// Solo/Unsolo a track based on `solo_tracks`.
    pub fn toggle_solo(
        &mut self,
        id: String,
        modifier_pressed: bool,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) {
        if let Some(track) = self.tracks.get_mut(&id) {
            if modifier_pressed {
                if self.solo_tracks.contains(&track.id) {
                    self.solo_tracks.retain(|t| *t != track.id);
                } else {
                    self.solo_tracks.push(track.id.clone());
                }
            } else {
                self.solo_tracks = if self.solo_tracks.contains(&track.id) {
                    vec![]
                } else {
                    vec![track.id.clone()]
                }
            }
            let _ = tx.push(GuiToPlayerMsg::SoloTracks(self.solo_tracks.clone()));
        }
    }
    /// Select a track
    pub fn select(&mut self, id: &String) {
        self.selected_tracks = vec![id.clone()];
    }
    /// Returns selected tracks
    pub fn selected_track(&self) -> Option<TrackReferenceCore> {
        if self.selected_tracks.len() > 0 {
            self.tracks.get(&self.selected_tracks[0]).map(|t| {
                t.get_reference(
                    self.order
                        .iter()
                        .position(|t_id| *t_id == t.id)
                        .unwrap_or(0),
                    true,
                    if self.solo_tracks.is_empty() {
                        TrackSoloState::NotSoloing
                    } else if self.solo_tracks.contains(&t.id) {
                        TrackSoloState::Solo
                    } else {
                        TrackSoloState::Soloing
                    },
                )
            })
        } else {
            None
        }
    }
    /// Move a clip identified by `id` to given track and position. All overlaps are fixed in the tracks.
    /// Returns created clips and deleted clips during the process.
    pub fn move_clip(
        &mut self,
        id: &String,
        to_track: &String,
        to_pos: f32,
        bpm: f32,
        ignore: &Vec<String>,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) -> (Vec<ClipCore>, Vec<ClipCore>) {
        let Some(old_track) = self._track_from_clip_id(id) else {
            return (Vec::new(), Vec::new());
        };

        let mut created_clips = Vec::new();
        let mut deleted_clips = Vec::new();
        // If moving within the same track, just update position
        if old_track.id == *to_track {
            let clip = if let Some(clip) = old_track.clips.iter_mut().find(|c| c.id == *id) {
                clip.position = to_pos;
                clip.clone()
            } else {
                return (Vec::new(), Vec::new());
            };
            old_track._fix_overlaps(&clip, bpm, &mut deleted_clips, &mut created_clips, ignore);
        } else {
            // Remove clips from its old track
            let new_clip = if let Some(old_track) = self._track_from_clip_id(id)
                && let Some(index) = old_track.clips.iter().position(|clip| clip.id == *id)
            {
                let clip = Some(old_track.clips[index].clone());
                old_track.clips.remove(index);
                clip
            } else {
                None
            };

            // Insert clip into new track if found
            if let (Some(mut clip), Some(new_track)) = (new_clip, self.tracks.get_mut(to_track)) {
                clip.position = to_pos;
                new_track._fix_overlaps(&clip, bpm, &mut deleted_clips, &mut created_clips, ignore);
                new_track.clips.push(clip);
            }
        }

        // Update audio thread
        if created_clips.len() > 0 {
            let mut map = HashMap::new();
            map.insert(to_track.clone(), created_clips.clone());
            let _ = tx.push(GuiToPlayerMsg::AddClips(map));
        }
        if deleted_clips.len() > 0 {
            let _ = tx.push(GuiToPlayerMsg::RemoveClip(
                deleted_clips.iter().map(|c| c.id.clone()).collect(),
            ));
        }
        let _ = tx.push(GuiToPlayerMsg::MoveClip(
            id.clone(),
            to_track.clone(),
            to_pos,
        ));

        (created_clips, deleted_clips)
    }

    /// Move a clip identified by `id` to given track and position.
    /// Use this function if you are sure no overlap can happen when moving the track.
    pub fn move_clip_skip_overlap_check(
        &mut self,
        id: &String,
        to_track: &String,
        to_pos: f32,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) {
        let Some(old_track) = self._track_from_clip_id(id) else {
            return;
        };

        // If moving within the same track, just update position
        if old_track.id == *to_track {
            if let Some(clip) = old_track.clips.iter_mut().find(|c| c.id == *id) {
                clip.position = to_pos;
            }
        } else {
            // Remove clips from its old track
            let new_clip = if let Some(old_track) = self._track_from_clip_id(id)
                && let Some(index) = old_track.clips.iter().position(|clip| clip.id == *id)
            {
                let clip = Some(old_track.clips[index].clone());
                old_track.clips.remove(index);
                clip
            } else {
                None
            };

            // Insert clip into new track if found
            if let (Some(mut clip), Some(new_track)) = (new_clip, self.tracks.get_mut(to_track)) {
                clip.position = to_pos;
                new_track.clips.push(clip);
            }
        }
        let _ = tx.push(GuiToPlayerMsg::MoveClip(
            id.clone(),
            to_track.clone(),
            to_pos,
        ));
    }

    pub fn resize_clip_skip_overlap_check(
        &mut self,
        id: &String,
        start: f32,
        end: f32,
        pos: f32,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) -> Option<(&mut TrackCore, ClipCore, ClipCore)> {
        let Some(track) = self._track_from_clip_id(id) else {
            return None;
        };

        let (old_clip, new_clip) = if let Some(clip) = track.clips.iter_mut().find(|c| c.id == *id)
        {
            let old_clip = clip.clone();
            clip.trim_start = start;
            clip.trim_end = end;
            clip.position = pos;
            // Update audio thread
            let _ = tx.push(GuiToPlayerMsg::ResizeClip(
                clip.clone().id,
                clip.trim_start,
                clip.trim_end,
                clip.position,
            ));
            (old_clip, clip.clone())
        } else {
            return None;
        };

        Some((track, old_clip, new_clip))
    }

    /// Update clip with new trim_start, trim_end and position
    pub fn resize_clip(
        &mut self,
        id: &String,
        start: f32,
        end: f32,
        pos: f32,
        bpm: f32,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) -> Option<(
        ClipCore,
        TrackCore,
        HashMap<String, Vec<ClipCore>>,
        Vec<ClipCore>,
    )> {
        let Some((track, old_clip, new_clip)) =
            self.resize_clip_skip_overlap_check(id, start, end, pos, tx)
        else {
            return None;
        };
        // Fix overlaps
        let mut deleted_clips = Vec::new();
        let mut created_clips = Vec::new();
        track._fix_overlaps(
            &new_clip,
            bpm,
            &mut deleted_clips,
            &mut created_clips,
            &Vec::new(),
        );
        let mut added_clips = HashMap::new();
        if !created_clips.is_empty() {
            added_clips.insert(track.id.clone(), created_clips);
            let _ = tx.push(GuiToPlayerMsg::AddClips(added_clips.clone()));
        }
        if !deleted_clips.is_empty() {
            let _ = tx.push(GuiToPlayerMsg::RemoveClip(
                deleted_clips.iter().map(|c| c.id.clone()).collect(),
            ));
        }

        Some((old_clip, track.clone(), added_clips, deleted_clips))
    }

    // TODO: Fix copy also effects
    /// Duplicate track identified by `id`, copying all attributes, clips and effects. The new track id is returned.
    pub fn duplicate(&mut self, id: &String, tx: &mut Producer<GuiToPlayerMsg>) -> Option<String> {
        // Find the index of the track to duplicate
        let index = self.order.iter().position(|o_id| o_id == id)?;
        let track = self.tracks.get(id)?;

        // Duplicate the track
        let (new_track, clip_map) = track.duplicate();
        let old_id = track.id.clone();
        let new_id = new_track.id.clone();

        // Insert the new track into tracks and order
        self.tracks.insert(new_id.clone(), new_track);
        self.order.insert(index + 1, new_id.clone());

        // Update audio thread
        let _ = tx.push(GuiToPlayerMsg::DuplicateTrack {
            id: old_id,
            new_id: new_id.clone(),
            clip_map,
        });

        Some(new_id)
    }

    pub fn add_clips_skip_overlap_check(
        &mut self,
        clips: HashMap<String, Vec<ClipCore>>,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) {
        for (track_id, clips) in clips {
            if let Some(track) = self.tracks.get_mut(&track_id) {
                track.add_clips_skip_overlap_check(clips, tx);
            }
        }
    }

    pub fn _track_from_clip_id(&mut self, id: &String) -> Option<&mut TrackCore> {
        for track in self.tracks.values_mut() {
            for clip in track.clips.iter() {
                if clip.id == *id {
                    return Some(track);
                }
            }
        }
        None
    }
}
