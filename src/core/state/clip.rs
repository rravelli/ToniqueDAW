use crate::core::{
    clip::ClipCore,
    state::{
        ToniqueProjectState,
        action::{
            AddClipsAction, CutClipAction, DeleteClipsAction, DuplicateClipAction, MoveClipAction,
            ResizeClipAction,
        },
    },
};

impl ToniqueProjectState {
    /// Add clips and fix all overlaps on the track.
    pub fn add_clips(&mut self, track_id: &String, clips: Vec<ClipCore>) {
        let action = AddClipsAction::new(clips, track_id);
        self.apply_action(Box::new(action));
    }
    /// Move clip to a new position and a new track fixing all overlaps on this track.
    pub fn move_clip(&mut self, id: &String, to_track: &String, to_pos: f32, ignore: &Vec<String>) {
        let action = MoveClipAction::new(id, to_track, to_pos, ignore);
        self.apply_action(Box::new(action));
    }
    /// Delete clips for their ids
    pub fn delete_clips(&mut self, ids: &Vec<String>) {
        let action = DeleteClipsAction::new(ids);
        self.apply_action(Box::new(action));
    }
    /// Cut clip located at position on given track. Does nothing it there is no clip.
    pub fn cut_clip_at(&mut self, track_id: &String, position: f32) {
        let action = CutClipAction::new(track_id, position);
        self.apply_action(Box::new(action));
    }
    /// Duplicate clips fixing all overlaps on the tracks.
    pub fn duplicate_clips(&mut self, ids: &Vec<String>, bounds: Option<(f32, f32)>) {
        let action = DuplicateClipAction::new(ids, bounds);
        self.apply_action(Box::new(action));
    }
    /// Resize clip without computing overlap checks.
    /// Use `commit_resize_clip` to apply overlap checks and add to undo stack.
    pub fn resize_clip(&mut self, id: &str, start: f32, end: f32, pos: f32) {
        // self.track_service
        //     .resize_clip_skip_overlap_check(id, start, end, pos, &mut self.tx);
        self.editor.resized_clip = Some((id.to_string(), start, end, pos));
    }
    /// Resize clip and perform overlap checks
    pub fn commit_resize_clip(&mut self, id: &str, start: f32, end: f32, pos: f32) {
        self.editor.resized_clip = None;
        let action = ResizeClipAction::new(id, start, end, pos);
        self.apply_action(Box::new(action));
    }
}
