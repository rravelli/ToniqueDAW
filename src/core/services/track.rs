use crate::{
    core::{
        clip::ClipCore,
        track::{MutableTrackCore, TrackCore, TrackReferenceCore, TrackSoloState},
    },
    message::GuiToPlayerMsg,
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

    pub fn get_mut(&mut self, id: String) -> &mut MutableTrackCore {
        self.tracks.get_mut(&id).unwrap().get_mutable_fields()
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
            .filter_map(|id| self.tracks.get(id))
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
    pub fn insert(&mut self, track: TrackCore, index: usize, tx: &mut Producer<GuiToPlayerMsg>) {
        let _ = tx.push(GuiToPlayerMsg::AddTrack(track.id.clone()));
        self.order.insert(index, track.id.clone());
        self.tracks.insert(track.id.clone(), track);
    }
    pub fn delete(&mut self, id: &String, tx: &mut Producer<GuiToPlayerMsg>) {
        if let Some(pos) = self.order.iter().position(|x| *x == *id) {
            self.order.remove(pos);
            self.selected_tracks.retain(|sel| sel != id);
            self.solo_tracks.retain(|sel| sel != id);
            self.tracks.remove(id);
            let _ = tx.push(GuiToPlayerMsg::RemoveTrack(id.clone()));
        }
    }
    /**
    Remove clips if their id is in given ids
     */
    pub fn delete_clips(&mut self, ids: &Vec<String>, tx: &mut Producer<GuiToPlayerMsg>) {
        for track in self.tracks.values_mut() {
            track.clips.retain(|clip| !ids.contains(&clip.id));
        }
        let _ = tx.push(GuiToPlayerMsg::RemoveClip(ids.clone()));
    }
    pub fn duplicate_clips(
        &mut self,
        ids: &Vec<String>,
        bounds: Option<(f32, f32)>,
        bpm: f32,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) -> Vec<ClipCore> {
        let mut new_clips = Vec::new();
        for track in self.tracks.values_mut() {
            new_clips.extend(track.duplicate_clips(ids, bounds, bpm, tx));
        }
        new_clips
    }

    pub fn set_volume(&mut self, id: String, volume: f32, tx: &mut Producer<GuiToPlayerMsg>) {
        if let Some(track) = self.tracks.get_mut(&id) {
            track.volume = volume;
            let _ = tx.push(GuiToPlayerMsg::ChangeTrackVolume(id, volume));
        }
    }
    pub fn set_mute(&mut self, id: String, mute: bool, tx: &mut Producer<GuiToPlayerMsg>) {
        if let Some(track) = self.tracks.get_mut(&id) {
            track.muted = mute;
            let _ = tx.push(GuiToPlayerMsg::MuteTrack(id, mute));
        }
    }
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
    pub fn select(&mut self, id: &String) {
        self.selected_tracks = vec![id.clone()];
    }
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

    pub fn move_clip(
        &mut self,
        id: &String,
        to_track: &String,
        to_pos: f32,
        bpm: f32,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) {
        // Try to find the current track containing the clip
        let Some(old_track) = self._track_from_clip_id(id) else {
            return;
        };

        let mut created_clips = Vec::new();
        let mut deleted_clips = Vec::new();
        // If moving within the same track, just update position
        if old_track.id == *to_track {
            let clip = if let Some(clip) = old_track.clips.iter_mut().find(|c| c.id == *id) {
                clip.position = to_pos;
                clip.clone()
            } else {
                return;
            };
            old_track._fix_overlaps(&clip, bpm, &mut deleted_clips, &mut created_clips);
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
                new_track._fix_overlaps(&clip, bpm, &mut deleted_clips, &mut created_clips);
                new_track.clips.push(clip);
            }
        }

        // Update audio thread
        if created_clips.len() > 0 {
            let _ = tx.push(GuiToPlayerMsg::AddClips(created_clips));
        }
        if deleted_clips.len() > 0 {
            let _ = tx.push(GuiToPlayerMsg::RemoveClip(deleted_clips));
        }
        let _ = tx.push(GuiToPlayerMsg::MoveClip(
            id.clone(),
            to_track.clone(),
            to_pos,
        ));
    }

    pub fn resize_clip(
        &mut self,
        id: &String,
        start: f32,
        end: f32,
        pos: f32,
        bpm: f32,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) {
        let Some(track) = self._track_from_clip_id(id) else {
            return;
        };

        let clip = if let Some(clip) = track.clips.iter_mut().find(|c| c.id == *id) {
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
            clip.clone()
        } else {
            return;
        };
        let mut deleted_clips = Vec::new();
        let mut created_clips = Vec::new();
        track._fix_overlaps(&clip, bpm, &mut deleted_clips, &mut created_clips);
        let _ = tx.push(GuiToPlayerMsg::AddClips(created_clips));
        let _ = tx.push(GuiToPlayerMsg::RemoveClip(deleted_clips));
    }

    // TODO: Fix copy also effects
    pub fn duplicate(&mut self, id: &String, tx: &mut Producer<GuiToPlayerMsg>) {
        if let Some(index) = self.order.iter().position(|o_id| o_id == id) {
            let Some(track) = self.tracks.get(id) else {
                return;
            };

            let (new_track, map, old_id) = {
                let (new_track, map) = track.duplicate();
                (new_track, map, track.id.clone())
            };

            let new_id = new_track.id.clone();
            self.tracks.insert(new_id.clone(), new_track);
            self.order.insert(index + 1, new_id.clone());

            let _ = tx.push(GuiToPlayerMsg::DuplicateTrack {
                id: old_id,
                new_id: new_id.clone(),
                clip_map: map,
            });
        }
    }

    fn _track_from_clip_id(&mut self, id: &String) -> Option<&mut TrackCore> {
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
