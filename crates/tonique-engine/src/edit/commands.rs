//! Undoable edit operations. Each command stores what it needs to revert
//! itself and reports its [`Effects`]: whether the graph must be rebuilt,
//! or only an automation curve swapped, or nothing (atomic param changes).

use super::{Bus, BusId, ChannelRef, Clip, ClipContent, ClipId, Edit, EditError, Effects, Output, Plugin, PluginId, Send, Track, TrackId};
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
    to_track: TrackId,
}

impl MoveClip {
    pub fn new(track: TrackId, id: ClipId, to: BeatPos) -> Self {
        Self { track, id, to, to_track: track }
    }

    /// Move the clip to `to` on another track.
    pub fn to_track(track: TrackId, id: ClipId, to_track: TrackId, to: BeatPos) -> Self {
        Self { track, id, to, to_track }
    }

    fn swap(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        edit.track(self.to_track)?;
        let t = edit.track_mut(self.track)?;
        let i = t.clips.iter().position(|c| c.id == self.id).ok_or(EditError::ClipNotFound(self.id))?;
        if self.to_track == self.track {
            std::mem::swap(&mut t.clips[i].start, &mut self.to);
        } else {
            let mut clip = t.clips.remove(i);
            std::mem::swap(&mut clip.start, &mut self.to);
            edit.track_mut(self.to_track)?.clips.push(clip);
            std::mem::swap(&mut self.track, &mut self.to_track);
        }
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

/// Change a clip's region: where it starts, how long it is, and (for audio
/// clips) where in the source it starts reading. Used for trimming.
pub struct ResizeClip {
    track: TrackId,
    id: ClipId,
    start: BeatPos,
    length: f64,
    source_offset_s: f64,
}

impl ResizeClip {
    /// `source_offset_s` is ignored for MIDI clips.
    pub fn new(track: TrackId, id: ClipId, start: BeatPos, length: f64, source_offset_s: f64) -> Self {
        Self { track, id, start, length, source_offset_s }
    }

    fn swap(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        if self.length <= 0.0 {
            return Err(EditError::Invalid("clip length must be positive"));
        }
        let t = edit.track_mut(self.track)?;
        let c = t.clips.iter_mut().find(|c| c.id == self.id).ok_or(EditError::ClipNotFound(self.id))?;
        std::mem::swap(&mut c.start, &mut self.start);
        std::mem::swap(&mut c.length, &mut self.length);
        if let ClipContent::Audio { source_offset_s, .. } = &mut c.content {
            std::mem::swap(source_offset_s, &mut self.source_offset_s);
        }
        Ok(Effects::rebuild())
    }
}

impl EditCommand for ResizeClip {
    fn label(&self) -> &'static str {
        "Resize clip"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
}

/// Reorder a track. Moving tracks never changes the sound.
pub struct MoveTrack {
    id: TrackId,
    index: usize,
}

impl MoveTrack {
    pub fn new(id: TrackId, index: usize) -> Self {
        Self { id, index }
    }

    fn swap(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let from = edit.tracks.iter().position(|t| t.id == self.id).ok_or(EditError::TrackNotFound(self.id))?;
        let t = edit.tracks.remove(from);
        let to = self.index.min(edit.tracks.len());
        edit.tracks.insert(to, t);
        self.index = from;
        Ok(Effects::none())
    }
}

impl EditCommand for MoveTrack {
    fn label(&self) -> &'static str {
        "Move track"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
}

pub struct RenameTrack {
    id: TrackId,
    name: String,
}

impl RenameTrack {
    pub fn new(id: TrackId, name: impl Into<String>) -> Self {
        Self { id, name: name.into() }
    }

    fn swap(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        std::mem::swap(&mut edit.track_mut(self.id)?.name, &mut self.name);
        Ok(Effects::none())
    }
}

impl EditCommand for RenameTrack {
    fn label(&self) -> &'static str {
        "Rename track"
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
    target: ChannelRef,
    soloed: bool,
}

impl SetSolo {
    pub fn new(track: TrackId, soloed: bool) -> Self {
        Self { target: ChannelRef::Track(track), soloed }
    }

    /// Solo a bus, and so everything routed into it.
    pub fn bus(bus: BusId, soloed: bool) -> Self {
        Self { target: ChannelRef::Bus(bus), soloed }
    }

    fn swap(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let soloed = match self.target {
            ChannelRef::Track(id) => &mut edit.track_mut(id)?.soloed,
            ChannelRef::Bus(id) => &mut edit.bus_mut(id)?.soloed,
            ChannelRef::Master => return Err(EditError::Invalid("the master can't be soloed")),
        };
        std::mem::swap(soloed, &mut self.soloed);
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

/// Route a bus into another bus or the master. Refused when the target is
/// the bus itself or routes into it: that would feed the bus into itself.
pub struct SetBusOutput {
    bus: BusId,
    output: Output,
}

impl SetBusOutput {
    pub fn new(bus: BusId, output: Output) -> Self {
        Self { bus, output }
    }

    fn swap(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        edit.bus(self.bus)?;
        if let Output::Bus(target) = self.output {
            edit.bus(target)?;
            if target == self.bus || edit.buses_along(Output::Bus(target)).contains(&self.bus) {
                return Err(EditError::RoutingCycle(self.bus));
            }
        }
        std::mem::swap(&mut edit.bus_mut(self.bus)?.output, &mut self.output);
        edit.refresh_mute_gains();
        Ok(Effects::rebuild())
    }
}

impl EditCommand for SetBusOutput {
    fn label(&self) -> &'static str {
        "Route bus"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
}

/// Remove a bus nothing routes or sends into anymore.
pub struct RemoveBus {
    id: BusId,
    removed: Option<(usize, Bus)>,
}

impl RemoveBus {
    pub fn new(id: BusId) -> Self {
        Self { id, removed: None }
    }
}

impl EditCommand for RemoveBus {
    fn label(&self) -> &'static str {
        "Remove bus"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let target = Output::Bus(self.id);
        let in_use = edit.tracks.iter().any(|t| t.output == target || t.sends.iter().any(|s| s.bus == self.id))
            || edit.buses.iter().any(|b| b.output == target);
        if in_use {
            return Err(EditError::Invalid("bus still has inputs"));
        }
        let i = edit.buses.iter().position(|b| b.id == self.id).ok_or(EditError::BusNotFound(self.id))?;
        self.removed = Some((i, edit.buses.remove(i)));
        edit.refresh_mute_gains();
        Ok(Effects::rebuild())
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        let (i, bus) = self.removed.take().ok_or(TAKEN)?;
        edit.buses.insert(i.min(edit.buses.len()), bus);
        edit.refresh_mute_gains();
        Ok(Effects::rebuild())
    }
}

pub struct RenameBus {
    id: BusId,
    name: String,
}

impl RenameBus {
    pub fn new(id: BusId, name: impl Into<String>) -> Self {
        Self { id, name: name.into() }
    }

    fn swap(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        std::mem::swap(&mut edit.bus_mut(self.id)?.name, &mut self.name);
        // Node labels carry the name.
        Ok(Effects::rebuild())
    }
}

impl EditCommand for RenameBus {
    fn label(&self) -> &'static str {
        "Rename bus"
    }
    fn apply(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
    fn revert(&mut self, edit: &mut Edit) -> Result<Effects, EditError> {
        self.swap(edit)
    }
}
