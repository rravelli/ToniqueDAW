//! The document model: an [`Edit`] holds tracks, buses, clips, plugins and
//! automation. It is plain owned data living on the control thread; the
//! [`builder`] turns it into a graph and [`EditSession`] keeps the engine in
//! sync as it's edited through undoable [`commands`].

pub mod builder;
pub mod commands;
mod session;
mod undo;

use std::collections::HashMap;
use std::sync::Arc;

pub use session::EditSession;
pub use undo::{Effects, UndoManager};

use crate::automation::BeatPoint;
use crate::meter::{ChannelMeter, PluginTap};
use crate::nodes::{Envelope, FilterMode};
use crate::param::{AtomicParam, ParamId};
use crate::sample::SampleBuffer;
use crate::time::{BeatPos, TempoMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TrackId(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BusId(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClipId(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PluginId(pub u64);
/// Audio data referenced by clips. See [`Edit::set_source`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceId(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub enum EditError {
    TrackNotFound(TrackId),
    BusNotFound(BusId),
    ClipNotFound(ClipId),
    PluginNotFound(PluginId),
    ParamNotFound(ParamId),
    /// Routing a bus there would feed it into itself.
    RoutingCycle(BusId),
    Invalid(&'static str),
    Engine(String),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for EditError {}

/// An automatable parameter. The value lives in a shared [`AtomicParam`], so
/// setting it is heard immediately without rebuilding the graph.
#[derive(Clone, Debug)]
pub struct Parameter {
    pub id: ParamId,
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub value: Arc<AtomicParam>,
    /// Automation in musical time; empty means not automated.
    pub automation: Vec<BeatPoint>,
}

/// Ramp applied to manual parameter changes, so UI moves never zipper.
pub const PARAM_RAMP_MS: f32 = 20.0;

impl Parameter {
    pub fn new(id: ParamId, name: &'static str, min: f32, max: f32, default: f32) -> Self {
        Self {
            id,
            name,
            min,
            max,
            default,
            value: Arc::new(AtomicParam::new(default)),
            automation: Vec::new(),
        }
    }

    pub fn get(&self) -> f32 {
        self.value.get()
    }

    fn duplicate(&self, edit: &mut Edit) -> Self {
        let mut p = Self::new(
            ParamId(edit.next_id()),
            self.name,
            self.min,
            self.max,
            self.default,
        );
        p.value.set(self.get(), 0.0);
        p.automation = self.automation.clone();
        p
    }

    /// Set (clamped) with a short ramp. Not undoable; see `commands::SetParam`.
    pub fn set(&self, v: f32) {
        self.value.set(v.clamp(self.min, self.max), PARAM_RAMP_MS);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PluginKind {
    Synth(Envelope),
    Filter(FilterMode),
    /// Echo with a fixed delay time (seconds).
    Echo {
        time_s: f32,
    },
    /// Stand-in for a lookahead plugin: a pure delay that reports latency.
    Latency {
        samples: usize,
    },
    /// Passes audio through untouched: a point in the chain for its
    /// [`Plugin::tap`] to show, like a spectrum analyser.
    Analyzer,
}

#[derive(Clone, Debug)]
pub struct Plugin {
    pub id: PluginId,
    pub kind: PluginKind,
    pub params: Vec<Parameter>,
    pub bypassed: bool,
    /// Given by the user; `None` to go by its kind.
    pub name: Option<String>,
    /// Its input and output, recorded while watched.
    pub tap: Arc<PluginTap>,
}

impl Plugin {
    pub fn new(edit: &mut Edit, kind: PluginKind) -> Self {
        let mut p = |name, min, max, default| {
            Parameter::new(ParamId(edit.next_id()), name, min, max, default)
        };
        let params = match kind {
            PluginKind::Synth(_) => vec![p("gain", 0.0, 2.0, 1.0)],
            PluginKind::Filter(_) => {
                vec![p("cutoff", 20.0, 20000.0, 2000.0), p("q", 0.1, 10.0, 0.707)]
            }
            PluginKind::Echo { .. } => vec![p("feedback", 0.0, 0.95, 0.4), p("mix", 0.0, 1.0, 0.3)],
            PluginKind::Latency { .. } | PluginKind::Analyzer => vec![],
        };
        Self {
            id: PluginId(edit.next_id()),
            kind,
            params,
            bypassed: false,
            name: None,
            tap: Arc::default(),
        }
    }

    pub fn param(&self, name: &str) -> Option<&Parameter> {
        self.params.iter().find(|p| p.name == name)
    }

    fn duplicate(&self, edit: &mut Edit) -> Self {
        let params = self.params.iter().map(|p| p.duplicate(edit)).collect();
        Self {
            id: PluginId(edit.next_id()),
            kind: self.kind,
            params,
            bypassed: self.bypassed,
            name: self.name.clone(),
            tap: Arc::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    /// Relative to the clip start, in beats.
    pub start: f64,
    pub length: f64,
    pub pitch: u8,
    pub velocity: u8,
}

#[derive(Clone, Debug)]
pub enum ClipContent {
    /// Plays silence until the source's data is set.
    Audio {
        source: SourceId,
        source_offset_s: f64,
        gain: f32,
    },
    Midi {
        notes: Vec<Note>,
        channel: u8,
    },
}

#[derive(Clone, Debug)]
pub struct Clip {
    pub id: ClipId,
    pub start: BeatPos,
    /// Length in beats.
    pub length: f64,
    pub fade_in_s: f64,
    pub fade_out_s: f64,
    pub content: ClipContent,
}

impl Clip {
    pub fn midi(edit: &mut Edit, start: BeatPos, length: f64, notes: Vec<Note>) -> Self {
        Self {
            id: ClipId(edit.next_id()),
            start,
            length,
            fade_in_s: 0.0,
            fade_out_s: 0.0,
            content: ClipContent::Midi { notes, channel: 0 },
        }
    }

    pub fn audio(edit: &mut Edit, start: BeatPos, length: f64, source: SourceId) -> Self {
        Self {
            id: ClipId(edit.next_id()),
            start,
            length,
            fade_in_s: 0.005,
            fade_out_s: 0.005,
            content: ClipContent::Audio {
                source,
                source_offset_s: 0.0,
                gain: 1.0,
            },
        }
    }

    pub fn end(&self) -> BeatPos {
        BeatPos(self.start.0 + self.length)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Master,
    Bus(BusId),
}

/// Post-fader send to a bus.
#[derive(Clone, Debug)]
pub struct Send {
    pub bus: BusId,
    pub level: Parameter,
    pan: Arc<AtomicParam>,
}

impl Send {
    pub fn new(edit: &mut Edit, bus: BusId, level: f32) -> Self {
        let level_param = Parameter::new(ParamId(edit.next_id()), "send", 0.0, 2.0, level);
        Self {
            bus,
            level: level_param,
            pan: Arc::new(AtomicParam::new(0.0)),
        }
    }
}

/// Fader section shared by tracks, buses and the master.
#[derive(Clone, Debug)]
pub struct Channel {
    pub plugins: Vec<Plugin>,
    pub volume: Parameter,
    pub pan: Parameter,
    pub muted: bool,
    /// Effective mute/solo gain, ramped; derived from mute/solo state.
    mute_gain: Arc<AtomicParam>,
    /// Post-fader levels, written by the audio thread.
    meter: Arc<ChannelMeter>,
}

impl Channel {
    /// Whether mute and solo let this channel through (as last refreshed).
    pub fn audible(&self) -> bool {
        self.mute_gain.get() > 0.5
    }

    fn new(edit: &mut Edit) -> Self {
        Self {
            plugins: Vec::new(),
            volume: Parameter::new(ParamId(edit.next_id()), "volume", 0.0, 2.0, 1.0),
            pan: Parameter::new(ParamId(edit.next_id()), "pan", -1.0, 1.0, 0.0),
            muted: false,
            mute_gain: Arc::new(AtomicParam::new(1.0)),
            meter: Arc::default(),
        }
    }

    /// Copy with fresh IDs and its own parameter values and meter, so the
    /// copy can be changed independently of the original.
    fn duplicate(&self, edit: &mut Edit) -> Self {
        let mut c = Self::new(edit);
        c.volume = self.volume.duplicate(edit);
        c.pan = self.pan.duplicate(edit);
        c.muted = self.muted;
        c.plugins = self.plugins.iter().map(|p| p.duplicate(edit)).collect();
        c
    }

    pub fn meter(&self) -> &ChannelMeter {
        &self.meter
    }

    pub fn plugin(&self, id: PluginId) -> Option<&Plugin> {
        self.plugins.iter().find(|p| p.id == id)
    }

    fn params(&self) -> impl Iterator<Item = &Parameter> {
        [&self.volume, &self.pan]
            .into_iter()
            .chain(self.plugins.iter().flat_map(|p| p.params.iter()))
    }

    fn params_mut(&mut self) -> impl Iterator<Item = &mut Parameter> {
        [&mut self.volume, &mut self.pan]
            .into_iter()
            .chain(self.plugins.iter_mut().flat_map(|p| p.params.iter_mut()))
    }
}

#[derive(Clone, Debug)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub clips: Vec<Clip>,
    pub channel: Channel,
    pub soloed: bool,
    pub output: Output,
    pub sends: Vec<Send>,
}

impl Track {
    pub fn new(edit: &mut Edit, name: impl Into<String>) -> Self {
        Self {
            id: TrackId(edit.next_id()),
            name: name.into(),
            clips: Vec::new(),
            channel: Channel::new(edit),
            soloed: false,
            output: Output::Master,
            sends: Vec::new(),
        }
    }

    pub fn clip(&self, id: ClipId) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == id)
    }

    /// Deep copy with fresh IDs throughout (track, clips, plugins, params).
    /// Sends are not copied.
    pub fn duplicate(&self, edit: &mut Edit) -> Self {
        let clips = self
            .clips
            .iter()
            .map(|c| Clip {
                id: ClipId(edit.next_id()),
                ..c.clone()
            })
            .collect();
        Self {
            id: TrackId(edit.next_id()),
            name: self.name.clone(),
            clips,
            channel: self.channel.duplicate(edit),
            soloed: false,
            output: self.output,
            sends: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Bus {
    pub id: BusId,
    pub name: String,
    pub channel: Channel,
    pub output: Output,
    /// Soloing a bus solos everything routed into it, however deep.
    pub soloed: bool,
}

impl Bus {
    pub fn new(edit: &mut Edit, name: impl Into<String>) -> Self {
        Self {
            id: BusId(edit.next_id()),
            name: name.into(),
            channel: Channel::new(edit),
            output: Output::Master,
            soloed: false,
        }
    }
}

/// Where a plugin chain lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelRef {
    Track(TrackId),
    Bus(BusId),
    Master,
}

#[derive(Clone, Debug)]
pub struct Edit {
    pub tempo: TempoMap,
    /// Explicit processing/display order.
    pub tracks: Vec<Track>,
    pub buses: Vec<Bus>,
    pub master: Channel,
    /// Metronome level (0 = off). Not part of the undo history.
    pub metronome: Arc<AtomicParam>,
    sources: HashMap<SourceId, Arc<SampleBuffer>>,
    next_id: u64,
}

impl Edit {
    pub fn new(bpm: f64) -> Self {
        let mut e = Self {
            tempo: TempoMap::new(bpm),
            tracks: Vec::new(),
            buses: Vec::new(),
            master: Channel::placeholder(),
            metronome: Arc::new(AtomicParam::new(0.0)),
            sources: HashMap::new(),
            next_id: 1,
        };
        e.master = Channel::new(&mut e);
        e
    }

    /// Fresh, never-reused ID (IDs stay unique across undo/redo).
    pub fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Reserve an ID for audio that isn't loaded yet; clips can use it
    /// right away and stay silent until [`Edit::set_source`].
    pub fn new_source(&mut self) -> SourceId {
        SourceId(self.next_id())
    }

    /// Register loaded audio and return its ID.
    pub fn add_source(&mut self, data: Arc<SampleBuffer>) -> SourceId {
        let id = self.new_source();
        self.sources.insert(id, data);
        id
    }

    /// Provide (or replace) a source's data. Takes effect on the next graph
    /// rebuild; [`EditSession::set_source`] does both. Data must be at the
    /// engine sample rate (see [`SampleBuffer::resampled`]).
    pub fn set_source(&mut self, id: SourceId, data: Arc<SampleBuffer>) {
        self.sources.insert(id, data);
    }

    pub fn source(&self, id: SourceId) -> Option<&Arc<SampleBuffer>> {
        self.sources.get(&id)
    }

    pub fn track(&self, id: TrackId) -> Result<&Track, EditError> {
        self.tracks
            .iter()
            .find(|t| t.id == id)
            .ok_or(EditError::TrackNotFound(id))
    }

    pub fn track_mut(&mut self, id: TrackId) -> Result<&mut Track, EditError> {
        self.tracks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(EditError::TrackNotFound(id))
    }

    pub fn bus(&self, id: BusId) -> Result<&Bus, EditError> {
        self.buses
            .iter()
            .find(|b| b.id == id)
            .ok_or(EditError::BusNotFound(id))
    }

    pub fn bus_mut(&mut self, id: BusId) -> Result<&mut Bus, EditError> {
        self.buses
            .iter_mut()
            .find(|b| b.id == id)
            .ok_or(EditError::BusNotFound(id))
    }

    /// The buses `output` leads through on its way to the master, nearest
    /// first. Stops on a broken or cyclic route.
    pub fn buses_along(&self, mut output: Output) -> Vec<BusId> {
        let mut path = Vec::new();
        while let Output::Bus(id) = output {
            let Ok(bus) = self.bus(id) else { break };
            if path.contains(&id) {
                break;
            }
            path.push(id);
            output = bus.output;
        }
        path
    }

    pub fn channel(&self, r: ChannelRef) -> Result<&Channel, EditError> {
        Ok(match r {
            ChannelRef::Track(id) => &self.track(id)?.channel,
            ChannelRef::Bus(id) => &self.bus(id)?.channel,
            ChannelRef::Master => &self.master,
        })
    }

    pub fn channel_mut(&mut self, r: ChannelRef) -> Result<&mut Channel, EditError> {
        Ok(match r {
            ChannelRef::Track(id) => &mut self.track_mut(id)?.channel,
            ChannelRef::Bus(id) => {
                &mut self
                    .buses
                    .iter_mut()
                    .find(|b| b.id == id)
                    .ok_or(EditError::BusNotFound(id))?
                    .channel
            }
            ChannelRef::Master => &mut self.master,
        })
    }

    fn all_params(&self) -> impl Iterator<Item = &Parameter> {
        self.tracks
            .iter()
            .flat_map(|t| t.channel.params().chain(t.sends.iter().map(|s| &s.level)))
            .chain(self.buses.iter().flat_map(|b| b.channel.params()))
            .chain(self.master.params())
    }

    pub fn param(&self, id: ParamId) -> Result<&Parameter, EditError> {
        self.all_params()
            .find(|p| p.id == id)
            .ok_or(EditError::ParamNotFound(id))
    }

    pub fn param_mut(&mut self, id: ParamId) -> Result<&mut Parameter, EditError> {
        self.tracks
            .iter_mut()
            .flat_map(|t| {
                t.channel
                    .params_mut()
                    .chain(t.sends.iter_mut().map(|s| &mut s.level))
            })
            .chain(self.buses.iter_mut().flat_map(|b| b.channel.params_mut()))
            .chain(self.master.params_mut())
            .find(|p| p.id == id)
            .ok_or(EditError::ParamNotFound(id))
    }

    /// Push mute/solo state into each channel's ramped mute gain. Cheap and
    /// graph-free: toggling mute or solo never rebuilds anything.
    pub fn refresh_mute_gains(&self) {
        let any_solo = self.tracks.iter().any(|t| t.soloed) || self.buses.iter().any(|b| b.soloed);
        for t in &self.tracks {
            // A soloed bus solos everything routed into it.
            let soloed = t.soloed
                || self
                    .buses_along(t.output)
                    .iter()
                    .any(|id| self.bus(*id).is_ok_and(|b| b.soloed));
            let audible = !t.channel.muted && (!any_solo || soloed);
            t.channel
                .mute_gain
                .set(if audible { 1.0 } else { 0.0 }, 10.0);
        }
        for c in self.buses.iter().map(|b| &b.channel).chain([&self.master]) {
            c.mute_gain.set(if c.muted { 0.0 } else { 1.0 }, 10.0);
        }
    }

    /// End of the last clip.
    pub fn length(&self) -> BeatPos {
        BeatPos(
            self.tracks
                .iter()
                .flat_map(|t| &t.clips)
                .map(|c| c.end().0)
                .fold(0.0, f64::max),
        )
    }
}

impl Channel {
    fn placeholder() -> Self {
        let p = Parameter::new(ParamId(0), "", 0.0, 0.0, 0.0);
        Self {
            plugins: Vec::new(),
            volume: p.clone(),
            pan: p,
            muted: false,
            mute_gain: Arc::new(AtomicParam::new(1.0)),
            meter: Arc::default(),
        }
    }
}
