//! Volume, pan, mute and solo.

use tonique_engine::edit::{
    BusId, Track, TrackId,
    commands::{SetMute, SetParam, SetSolo},
};

use super::ProjectState;

impl ProjectState {
    /// Set individual track volume. Changes are not saved in undo stack.
    pub fn set_volume(&mut self, id: TrackId, volume: f32) {
        if let Ok(c) = self.edit().channel(self.channel_ref(id)) {
            c.volume.set(volume);
        }
    }
    /// Set track volume and save in undo stack given `old_volume`.
    pub fn commit_volume(&mut self, id: TrackId, old_volume: f32, new_volume: f32) {
        let Ok(c) = self.edit().channel(self.channel_ref(id)) else {
            return;
        };
        // `SetParam` records the current value for undo: restore it first.
        c.volume.set(old_volume);
        let param = c.volume.id;
        self.perform(SetParam::new(param, new_volume));
    }
    /// Set track pan (-1 left to 1 right). Changes are not saved in undo
    /// stack.
    pub fn set_pan(&mut self, id: TrackId, pan: f32) {
        if let Ok(c) = self.edit().channel(self.channel_ref(id)) {
            c.pan.set(pan);
        }
    }
    /// Set track pan and save in undo stack given `old_pan`.
    pub fn commit_pan(&mut self, id: TrackId, old_pan: f32, new_pan: f32) {
        let Ok(c) = self.edit().channel(self.channel_ref(id)) else {
            return;
        };
        // `SetParam` records the current value for undo: restore it first.
        c.pan.set(old_pan);
        let param = c.pan.id;
        self.perform(SetParam::new(param, new_pan));
    }
    /// Mute or unmute this track
    pub fn set_mute(&mut self, id: TrackId, mute: bool) {
        self.perform(SetMute::new(self.channel_ref(id), mute));
    }
    /// Toggle the solo button.
    /// Toggle the solo button of a track or a group. Without the modifier,
    /// every other solo is cleared.
    pub fn toggle_solo(&mut self, id: TrackId, modifier_pressed: bool) {
        let edit = self.edit();
        let soloed: Vec<(TrackId, bool)> = edit
            .tracks
            .iter()
            .map(|t| (t.id, t.soloed))
            .chain(edit.buses.iter().map(|b| (TrackId(b.id.0), b.soloed)))
            .collect();
        let Some(current) = soloed.iter().find(|(t, _)| *t == id).map(|(_, s)| *s) else {
            return;
        };
        let changes: Vec<(TrackId, bool)> = soloed
            .into_iter()
            .filter_map(|(t, was)| {
                let solo = if t == id {
                    !current
                } else {
                    modifier_pressed && was
                };
                (solo != was).then_some((t, solo))
            })
            .collect();
        self.transaction("Solo", |s| {
            for (target, solo) in changes {
                if s.is_group(target) {
                    s.perform(SetSolo::bus(BusId(target.0), solo));
                } else {
                    s.perform(SetSolo::new(target, solo));
                }
            }
        });
    }
    /// Whether the engine lets `track` through, as it works it out (see
    /// `Edit::refresh_mute_gains`): neither it nor a group it's in muted,
    /// and, while anything is soloed, it or a group it's in soloed. Mute
    /// wins over solo.
    pub(super) fn track_heard(&self, track: &Track) -> bool {
        let edit = self.edit();
        let groups: Vec<_> = edit
            .buses_along(track.output)
            .into_iter()
            .filter_map(|b| edit.bus(b).ok())
            .collect();
        !track.channel.muted
            && !groups.iter().any(|g| g.channel.muted)
            && (!self.any_solo() || track.soloed || groups.iter().any(|g| g.soloed))
    }

    /// Whether any track or group is soloed.
    pub(super) fn any_solo(&self) -> bool {
        let edit = self.edit();
        edit.tracks.iter().any(|t| t.soloed) || edit.buses.iter().any(|b| b.soloed)
    }
}
