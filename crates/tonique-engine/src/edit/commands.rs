//! Undoable edit operations. Each command stores what it needs to revert
//! itself and reports its [`Effects`]: whether the graph must be rebuilt,
//! or only an automation curve swapped, or nothing (atomic param changes).

use super::{Bus, BusId, ChannelRef, Clip, ClipId, Edit, EditError, Effects, Output, Plugin, PluginId, Send, Track, TrackId};
use crate::automation::BeatPoint;
use crate::param::ParamId;
use crate::time::{BeatPos, TempoMap};

pub trait EditCommand: std::marker::Send {
    fn label(&self) -> &'static str;
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError>;
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError>;
}

const TAKEN: EditError = EditError::Invalid("command applied twice");

pub struct AddTrack {
    track: Option<Track>,
    id: TrackId,
    index: Option<usize>,
}

impl AddTrack {
    pub fn new(track: Track) -> Self {
        Self { id: track.id, track: Some(track), index: None }
    }

    pub fn at(track: Track, index: usize) -> Self {
        Self { index: Some(index), ..Self::new(track) }
    }
}

impl EditCommand for AddTrack {
    fn label(&self) -> &'static str {
        "Add track"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let t = self.track.take().ok_or(TAKEN)?;
        let i = self.index.unwrap_or(edit.tracks.len()).min(edit.tracks.len());
        edit.tracks.insert(i, t);
        self.index = Some(i);
        edit.refresh_mute_gains();
        Ok(Effects::rebuild())
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let i = edit.tracks.iter().position(|t| t.id == self.id).ok_or(EditError::TrackNotFound(self.id))?;
        self.track = Some(edit.tracks.remove(i));
        edit.refresh_mute_gains();
        Ok(Effects::rebuild())
    }
}

pub struct RemoveTrack {
    id: TrackId,
    removed: Option<(usize, Track)>,
}

impl RemoveTrack {
    pub fn new(id: TrackId) -> Self {
        Self { id, removed: None }
    }
}

impl EditCommand for RemoveTrack {
    fn label(&self) -> &'static str {
        "Remove track"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let i = edit.tracks.iter().position(|t| t.id == self.id).ok_or(EditError::TrackNotFound(self.id))?;
        self.removed = Some((i, edit.tracks.remove(i)));
        edit.refresh_mute_gains();
        Ok(Effects::rebuild())
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let (i, t) = self.removed.take().ok_or(TAKEN)?;
        edit.tracks.insert(i.min(edit.tracks.len()), t);
        edit.refresh_mute_gains();
        Ok(Effects::rebuild())
    }
}

pub struct AddClip {
    track: TrackId,
    id: ClipId,
    clip: Option<Clip>,
}

impl AddClip {
    pub fn new(track: TrackId, clip: Clip) -> Self {
        Self { track, id: clip.id, clip: Some(clip) }
    }
}

impl EditCommand for AddClip {
    fn label(&self) -> &'static str {
        "Add clip"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let clip = self.clip.take().ok_or(TAKEN)?;
        edit.track_mut(self.track)?.clips.push(clip);
        Ok(Effects::rebuild())
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let t = edit.track_mut(self.track)?;
        let i = t.clips.iter().position(|c| c.id == self.id).ok_or(EditError::ClipNotFound(self.id))?;
        self.clip = Some(t.clips.remove(i));
        Ok(Effects::rebuild())
    }
}

pub struct RemoveClip {
    track: TrackId,
    id: ClipId,
    removed: Option<(usize, Clip)>,
}

impl RemoveClip {
    pub fn new(track: TrackId, id: ClipId) -> Self {
        Self { track, id, removed: None }
    }
}

impl EditCommand for RemoveClip {
    fn label(&self) -> &'static str {
        "Remove clip"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let t = edit.track_mut(self.track)?;
        let i = t.clips.iter().position(|c| c.id == self.id).ok_or(EditError::ClipNotFound(self.id))?;
        self.removed = Some((i, t.clips.remove(i)));
        Ok(Effects::rebuild())
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let (i, c) = self.removed.take().ok_or(TAKEN)?;
        let t = edit.track_mut(self.track)?;
        t.clips.insert(i.min(t.clips.len()), c);
        Ok(Effects::rebuild())
    }
}

pub struct MoveClip {
    track: TrackId,
    id: ClipId,
    to: BeatPos,
}

impl MoveClip {
    pub fn new(track: TrackId, id: ClipId, to: BeatPos) -> Self {
        Self { track, id, to }
    }

    fn swap(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let t = edit.track_mut(self.track)?;
        let c = t.clips.iter_mut().find(|c| c.id == self.id).ok_or(EditError::ClipNotFound(self.id))?;
        std::mem::swap(&mut c.start, &mut self.to);
        Ok(Effects::rebuild())
    }
}

impl EditCommand for MoveClip {
    fn label(&self) -> &'static str {
        "Move clip"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
}

pub struct AddPlugin {
    target: ChannelRef,
    id: PluginId,
    index: Option<usize>,
    plugin: Option<Plugin>,
}

impl AddPlugin {
    pub fn new(target: ChannelRef, plugin: Plugin) -> Self {
        Self { target, id: plugin.id, index: None, plugin: Some(plugin) }
    }

    pub fn at(target: ChannelRef, plugin: Plugin, index: usize) -> Self {
        Self { index: Some(index), ..Self::new(target, plugin) }
    }
}

impl EditCommand for AddPlugin {
    fn label(&self) -> &'static str {
        "Add plugin"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let p = self.plugin.take().ok_or(TAKEN)?;
        let ch = edit.channel_mut(self.target)?;
        let i = self.index.unwrap_or(ch.plugins.len()).min(ch.plugins.len());
        ch.plugins.insert(i, p);
        self.index = Some(i);
        Ok(Effects::rebuild())
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let ch = edit.channel_mut(self.target)?;
        let i = ch.plugins.iter().position(|p| p.id == self.id).ok_or(EditError::PluginNotFound(self.id))?;
        self.plugin = Some(ch.plugins.remove(i));
        Ok(Effects::rebuild())
    }
}

pub struct RemovePlugin {
    target: ChannelRef,
    id: PluginId,
    removed: Option<(usize, Plugin)>,
}

impl RemovePlugin {
    pub fn new(target: ChannelRef, id: PluginId) -> Self {
        Self { target, id, removed: None }
    }
}

impl EditCommand for RemovePlugin {
    fn label(&self) -> &'static str {
        "Remove plugin"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let ch = edit.channel_mut(self.target)?;
        let i = ch.plugins.iter().position(|p| p.id == self.id).ok_or(EditError::PluginNotFound(self.id))?;
        self.removed = Some((i, ch.plugins.remove(i)));
        Ok(Effects::rebuild())
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let (i, p) = self.removed.take().ok_or(TAKEN)?;
        let ch = edit.channel_mut(self.target)?;
        ch.plugins.insert(i.min(ch.plugins.len()), p);
        Ok(Effects::rebuild())
    }
}

pub struct SetBypass {
    target: ChannelRef,
    id: PluginId,
    bypassed: bool,
}

impl SetBypass {
    pub fn new(target: ChannelRef, id: PluginId, bypassed: bool) -> Self {
        Self { target, id, bypassed }
    }

    fn swap(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let ch = edit.channel_mut(self.target)?;
        let p = ch.plugins.iter_mut().find(|p| p.id == self.id).ok_or(EditError::PluginNotFound(self.id))?;
        std::mem::swap(&mut p.bypassed, &mut self.bypassed);
        Ok(Effects::rebuild())
    }
}

impl EditCommand for SetBypass {
    fn label(&self) -> &'static str {
        "Bypass plugin"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
}

/// Undoable parameter change. Heard immediately through the atomic; no rebuild.
pub struct SetParam {
    id: ParamId,
    value: f32,
}

impl SetParam {
    pub fn new(id: ParamId, value: f32) -> Self {
        Self { id, value }
    }

    fn swap(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let p = edit.param(self.id)?;
        let old = p.get();
        p.set(self.value);
        self.value = old;
        Ok(Effects::none())
    }
}

impl EditCommand for SetParam {
    fn label(&self) -> &'static str {
        "Change parameter"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
}

pub struct SetMute {
    target: ChannelRef,
    muted: bool,
}

impl SetMute {
    pub fn new(target: ChannelRef, muted: bool) -> Self {
        Self { target, muted }
    }

    fn swap(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        std::mem::swap(&mut edit.channel_mut(self.target)?.muted, &mut self.muted);
        edit.refresh_mute_gains();
        Ok(Effects::none())
    }
}

impl EditCommand for SetMute {
    fn label(&self) -> &'static str {
        "Mute"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
}

pub struct SetSolo {
    track: TrackId,
    soloed: bool,
}

impl SetSolo {
    pub fn new(track: TrackId, soloed: bool) -> Self {
        Self { track, soloed }
    }

    fn swap(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        std::mem::swap(&mut edit.track_mut(self.track)?.soloed, &mut self.soloed);
        edit.refresh_mute_gains();
        Ok(Effects::none())
    }
}

impl EditCommand for SetSolo {
    fn label(&self) -> &'static str {
        "Solo"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
}

/// Replace a parameter's automation. If the parameter was already automated
/// (and stays so), the new curve is swapped into the running automation
/// node through the command ring instead of rebuilding the graph.
pub struct SetAutomation {
    id: ParamId,
    points: Vec<BeatPoint>,
}

impl SetAutomation {
    pub fn new(id: ParamId, points: Vec<BeatPoint>) -> Self {
        Self { id, points }
    }

    fn swap(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let p = edit.param_mut(self.id)?;
        std::mem::swap(&mut p.automation, &mut self.points);
        // `self.points` now holds the previous lane.
        Ok(if p.automation.is_empty() != self.points.is_empty() { Effects::rebuild() } else { Effects::curve(self.id) })
    }
}

impl EditCommand for SetAutomation {
    fn label(&self) -> &'static str {
        "Edit automation"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
}

/// Any tempo map change: clips and automation are re-placed in samples.
pub struct SetTempoMap {
    tempo: TempoMap,
}

impl SetTempoMap {
    pub fn new(tempo: TempoMap) -> Self {
        Self { tempo }
    }

    fn swap(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        std::mem::swap(&mut edit.tempo, &mut self.tempo);
        Ok(Effects::rebuild())
    }
}

impl EditCommand for SetTempoMap {
    fn label(&self) -> &'static str {
        "Change tempo"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
}

pub struct AddBus {
    id: BusId,
    bus: Option<Bus>,
}

impl AddBus {
    pub fn new(bus: Bus) -> Self {
        Self { id: bus.id, bus: Some(bus) }
    }
}

impl EditCommand for AddBus {
    fn label(&self) -> &'static str {
        "Add bus"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        edit.buses.push(self.bus.take().ok_or(TAKEN)?);
        Ok(Effects::rebuild())
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let i = edit.buses.iter().position(|b| b.id == self.id).ok_or(EditError::BusNotFound(self.id))?;
        self.bus = Some(edit.buses.remove(i));
        Ok(Effects::rebuild())
    }
}

pub struct SetOutput {
    track: TrackId,
    output: Output,
}

impl SetOutput {
    pub fn new(track: TrackId, output: Output) -> Self {
        Self { track, output }
    }

    fn swap(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        if let Output::Bus(b) = self.output {
            edit.bus(b)?;
        }
        std::mem::swap(&mut edit.track_mut(self.track)?.output, &mut self.output);
        Ok(Effects::rebuild())
    }
}

impl EditCommand for SetOutput {
    fn label(&self) -> &'static str {
        "Route track"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
}

pub struct AddSend {
    track: TrackId,
    send: Option<Send>,
    param: ParamId,
}

impl AddSend {
    pub fn new(track: TrackId, send: Send) -> Self {
        Self { track, param: send.level.id, send: Some(send) }
    }
}

impl EditCommand for AddSend {
    fn label(&self) -> &'static str {
        "Add send"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        edit.bus(self.send.as_ref().ok_or(TAKEN)?.bus)?;
        let track = edit.track_mut(self.track)?;
        track.sends.push(self.send.take().ok_or(TAKEN)?);
        Ok(Effects::rebuild())
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let t = edit.track_mut(self.track)?;
        let i = t.sends.iter().position(|s| s.level.id == self.param).ok_or(EditError::ParamNotFound(self.param))?;
        self.send = Some(t.sends.remove(i));
        Ok(Effects::rebuild())
    }
}
