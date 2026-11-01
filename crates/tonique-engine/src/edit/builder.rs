//! Edit -> graph description. Runs on the control thread.
//!
//! ```text
//!  clips ─▶ Σ ─▶ plugins… ─▶ fader(vol/pan/mute) ─┬─▶ Σ bus ─▶ plugins… ─▶ fader ─┐
//!                                                 └─▶ send ─▶ Σ bus              ▼
//!                                                          Σ master ─▶ plugins… ─▶ fader ─▶ out
//! ```
//!
//! Identities are derived from model IDs (track, plugin, clip, param), so
//! every rebuild maps onto the previous graph's instances and their state.

use std::collections::HashMap;
use std::sync::Arc;

use super::{
    Bus, BusId, Channel, ClipContent, ClipLoop, Edit, Output, Parameter, Plugin, PluginKind, Track,
};
use crate::automation::AutomationCurve;
use crate::graph::{GraphDescription, Node, NodeId, NodeIdentity};
use crate::meter::PluginTap;
use crate::nodes::{
    AudioClipNode, AutomationNode, ClipPlacement, ClipWarp, CompressorNode, DelayNode, EQ_BANDS,
    EchoNode, EqNode, FilterMode, FilterNode, GateNode, LimiterNode, LoopPlacement, MetronomeNode,
    MidiClipNode, ReverbNode, SaturationNode, SumNode, SynthNode, TappedNode, ThroughNode,
    TimelineNote, UtilityNode, VolumePanNode,
};
use crate::param::{AtomicParam, ParamId};
use crate::stretch::Stretchers;

/// Crossfade at a clip loop's seam: long enough not to click, short enough
/// not to smear its attack.
const LOOP_CROSSFADE_S: f64 = 0.004;
/// Longest echo, in seconds: a synced one at a slow tempo can ask for
/// more.
const MAX_ECHO_S: f32 = 8.0;
use crate::time::{BeatPos, SamplePos};

/// Identity of the automation writer for a parameter; used to address it
/// with [`crate::engine::Command::SendToNode`].
pub fn automation_identity(param: ParamId) -> NodeIdentity {
    NodeIdentity::of(&("automation", param.0))
}

pub fn build_graph(edit: &Edit, sample_rate: f64, stretchers: &mut Stretchers) -> GraphDescription {
    let mut b = Builder {
        edit,
        sr: sample_rate,
        stretchers,
        d: GraphDescription::new(),
        owner: None,
    };
    b.build()
}

/// Curve for a parameter, converted from beats through the tempo map.
pub fn automation_curve(edit: &Edit, param: &Parameter, sample_rate: f64) -> Arc<AutomationCurve> {
    Arc::new(AutomationCurve::from_beats(
        &param.automation,
        &edit.tempo,
        sample_rate,
    ))
}

struct Builder<'a> {
    edit: &'a Edit,
    sr: f64,
    stretchers: &'a mut Stretchers,
    d: GraphDescription,
    /// Track or bus whose nodes are being added (for inspection).
    owner: Option<u64>,
}

impl Builder<'_> {
    fn build(&mut self) -> GraphDescription {
        let master_in = self.d.add(SumNode::new(2), &[]);
        self.label(master_in, "master · Σ");
        let mut bus_in: HashMap<BusId, NodeId> = HashMap::new();
        for bus in &self.edit.buses {
            self.owner = Some(bus.id.0);
            let sum = self.d.add(SumNode::new(2), &[]);
            self.label(sum, format!("{} · Σ", bus.name));
            bus_in.insert(bus.id, sum);
        }
        let route = |o: Output| match o {
            Output::Master => master_in,
            Output::Bus(id) => bus_in.get(&id).copied().unwrap_or(master_in),
        };

        for track in &self.edit.tracks {
            self.owner = Some(track.id.0);
            let out = self.track(track);
            self.d.connect(out, route(track.output));
            for send in &track.sends {
                if let Some(&target) = bus_in.get(&send.bus) {
                    let id = NodeIdentity::of(&("send", track.id.0, send.level.id.0));
                    let node = VolumePanNode::new(send.level.value.clone(), send.pan.clone())
                        .with_identity(id);
                    let s = self.d.add(node, &[out]);
                    let bus_name = self.edit.bus(send.bus).map_or("bus", |b| b.name.as_str());
                    self.label(s, format!("{} → {bus_name}", track.name));
                    self.automate(&send.level, s, &track.name);
                    self.d.connect(s, target);
                }
            }
        }
        for bus in &self.edit.buses {
            self.owner = Some(bus.id.0);
            let out = self.bus(bus, bus_in[&bus.id]);
            self.d.connect(out, route(bus.output));
        }
        self.owner = None;
        let master = self.channel(
            &self.edit.master,
            master_in,
            NodeIdentity::of(&"master"),
            "master",
        );
        // The click bypasses the master fader and meter.
        let click = MetronomeNode::new(self.edit.tempo.clone(), self.edit.metronome.clone())
            .with_identity(NodeIdentity::of(&"metronome"));
        let click = self.d.add(click, &[]);
        self.label(click, "metronome");
        let out = self.d.add(SumNode::new(2), &[master, click]);
        self.label(out, "output");
        self.d.set_output(out);
        self.d.set_tempo(self.edit.tempo.clone());
        std::mem::take(&mut self.d)
    }

    fn label(&mut self, node: NodeId, label: impl Into<String>) {
        self.d.set_label(node, label);
        if let Some(owner) = self.owner {
            self.d.set_owner(node, owner);
        }
    }

    fn samples(&self, beat: f64) -> SamplePos {
        self.edit.tempo.beats_to_samples(BeatPos(beat), self.sr)
    }

    fn track(&mut self, track: &Track) -> NodeId {
        let mut sources = Vec::new();
        for clip in &track.clips {
            let start = self.samples(clip.start.0);
            let end = self.samples(clip.end().0);
            let node = match &clip.content {
                ClipContent::Audio {
                    source,
                    source_offset_s,
                    gain,
                    pan,
                    looped,
                    ..
                } => {
                    // Not loaded yet: silent until the source is set.
                    let Some(source) = self.edit.source(*source) else {
                        continue;
                    };
                    let place = ClipPlacement {
                        start,
                        length: end - start,
                        source_offset: (source_offset_s * self.sr).round() as SamplePos,
                        fade_in: (clip.fade_in_s * self.sr) as SamplePos,
                        fade_out: (clip.fade_out_s * self.sr) as SamplePos,
                        looped: looped.filter(ClipLoop::is_valid).map(|l| {
                            LoopPlacement::new(
                                (l.start_s * self.sr).round() as SamplePos,
                                (l.end_s * self.sr).round() as SamplePos,
                                (LOOP_CROSSFADE_S * self.sr) as SamplePos,
                            )
                        }),
                    };
                    let node = AudioClipNode::new(
                        source.clone(),
                        place,
                        gain.value.clone(),
                        pan.value.clone(),
                    );
                    match clip.warp(&self.edit.tempo).unwrap_or(ClipWarp::None) {
                        // Stretched: its stretcher, fed so far, kept across
                        // rebuilds, so its own identity.
                        warp @ ClipWarp::Stretch { settings, .. } => {
                            let stretcher = self.stretchers.get(clip.id.0, settings.window);
                            let node = node
                                .with_warp(warp, stretcher)
                                .with_identity(NodeIdentity::of(&("audio-clip", clip.id)));
                            self.d.add(node, &[])
                        }
                        // Stateless: no identity needed, and dedupable by
                        // content.
                        warp => self.d.add(node.with_warp(warp, None), &[]),
                    }
                }
                ClipContent::Midi { notes, channel } => {
                    let tl: Vec<TimelineNote> = notes
                        .iter()
                        .map(|n| {
                            let s = self.samples(clip.start.0 + n.start);
                            TimelineNote {
                                start: s,
                                length: self.samples(clip.start.0 + n.start + n.length) - s,
                                note: n.pitch,
                                velocity: n.velocity,
                            }
                        })
                        .collect();
                    let node = MidiClipNode::new(&tl, *channel, start, end)
                        .with_identity(NodeIdentity::of(&("clip", clip.id.0)));
                    self.d.add(node, &[])
                }
            };
            self.label(node, format!("{} · clip", track.name));
            sources.push(node);
        }
        let input = self.d.add(SumNode::new(2), &sources);
        self.label(input, format!("{} · clips Σ", track.name));
        self.channel(
            &track.channel,
            input,
            NodeIdentity::of(&("track", track.id.0)),
            &track.name,
        )
    }

    fn bus(&mut self, bus: &Bus, input: NodeId) -> NodeId {
        self.channel(
            &bus.channel,
            input,
            NodeIdentity::of(&("bus", bus.id.0)),
            &bus.name,
        )
    }

    /// Plugin chain + fader for a track, bus or the master.
    fn channel(
        &mut self,
        ch: &Channel,
        input: NodeId,
        fader_identity: NodeIdentity,
        name: &str,
    ) -> NodeId {
        let mut prev = input;
        for plugin in ch.plugins.iter().filter(|p| !p.bypassed) {
            let id = NodeIdentity::of(&("plugin", plugin.id.0));
            let param = |name| plugin.param(name).expect("plugin param").value.clone();
            let tap = plugin.tap.clone();
            prev = match plugin.kind {
                PluginKind::Synth(env) => self.tapped(
                    SynthNode::new(env, param("gain")).with_identity(id),
                    tap,
                    prev,
                ),
                PluginKind::Filter(mode) => {
                    let [cutoff, q, gain] = params(plugin, ["cutoff", "q", "gain"]);
                    let node = FilterNode::new(mode, cutoff, q, gain);
                    self.tapped(node.with_identity(id), tap, prev)
                }
                PluginKind::Echo { time_s, sync } => {
                    // Synced, at the tempo it starts at.
                    let time_s = sync.map_or(time_s, |beats| {
                        (beats as f64 * 60.0 / self.edit.tempo.bpm_at(BeatPos(0.0))) as f32
                    });
                    let [feedback, mix, low_cut, high_cut] =
                        params(plugin, ["feedback", "mix", "low_cut", "high_cut"]);
                    let time_s = time_s.clamp(0.001, MAX_ECHO_S);
                    let node = EchoNode::new(time_s, feedback, mix, low_cut, high_cut);
                    self.tapped(node.with_identity(id), tap, prev)
                }
                PluginKind::Utility => {
                    let [gain, balance, width, mono, invert_left, invert_right] = params(
                        plugin,
                        [
                            "gain",
                            "balance",
                            "width",
                            "mono",
                            "invert_left",
                            "invert_right",
                        ],
                    );
                    let node =
                        UtilityNode::new(gain, balance, width, mono, invert_left, invert_right);
                    self.tapped(node.with_identity(id), tap, prev)
                }
                PluginKind::Compressor => {
                    let p = params(
                        plugin,
                        ["threshold", "ratio", "attack", "release", "knee", "makeup"],
                    );
                    let node = CompressorNode::new(p, tap.clone());
                    self.tapped(node.with_identity(id), tap, prev)
                }
                PluginKind::Limiter { lookahead_ms } => {
                    let samples = (lookahead_ms as f64 * 0.001 * self.sr).round() as usize;
                    let p = params(plugin, ["gain", "ceiling", "release"]);
                    let node = LimiterNode::new(samples, p, tap.clone());
                    self.tapped(node.with_identity(id), tap, prev)
                }
                PluginKind::Eq => {
                    let node = EqNode::new(EQ_BANDS.map(|band| params(plugin, band.params)));
                    self.tapped(node.with_identity(id), tap, prev)
                }
                PluginKind::Reverb => {
                    let p = params(plugin, ["size", "decay", "damping", "predelay", "mix"]);
                    self.tapped(ReverbNode::new(p).with_identity(id), tap, prev)
                }
                PluginKind::Saturation(curve) => {
                    let node =
                        SaturationNode::new(curve, params(plugin, ["drive", "output", "mix"]));
                    self.tapped(node.with_identity(id), tap, prev)
                }
                PluginKind::Gate => {
                    let p = params(plugin, ["threshold", "range", "attack", "hold", "release"]);
                    let node = GateNode::new(p, tap.clone());
                    self.tapped(node.with_identity(id), tap, prev)
                }
                PluginKind::Analyzer => {
                    self.tapped(ThroughNode::new(2).with_identity(id), tap, prev)
                }
                PluginKind::Latency { samples } => {
                    self.tapped(DelayNode::reporting(samples, 2), tap, prev)
                }
            };
            self.label(prev, format!("{name} · {}", plugin_label(plugin.kind)));
            for p in &plugin.params {
                self.automate(p, prev, name);
            }
        }
        let fader = VolumePanNode::new(ch.volume.value.clone(), ch.pan.value.clone())
            .with_gain(ch.mute_gain.clone())
            .with_meter(ch.meter.clone())
            .with_identity(fader_identity);
        let fader = self.d.add(fader, &[prev]);
        self.label(fader, format!("{name} · fader"));
        self.automate(&ch.volume, fader, name);
        self.automate(&ch.pan, fader, name);
        fader
    }

    /// A plugin's node after `input`, recording into its tap.
    fn tapped<N: Node + 'static>(&mut self, node: N, tap: Arc<PluginTap>, input: NodeId) -> NodeId {
        self.d.add(TappedNode::new(node, tap), &[input])
    }

    /// Add an automation writer for `param` that runs before `owner`.
    fn automate(&mut self, param: &Parameter, owner: NodeId, owner_name: &str) {
        if param.automation.is_empty() {
            return;
        }
        let curve = automation_curve(self.edit, param, self.sr);
        let node = AutomationNode::new(curve, param.value.clone(), automation_identity(param.id));
        let a = self.d.add(node, &[]);
        self.label(a, format!("{owner_name} · {} automation", param.name));
        self.d.add_order_dependency(owner, a);
    }
}

/// The values of `plugin`'s parameters by those names.
fn params<const N: usize>(plugin: &Plugin, names: [&str; N]) -> [Arc<AtomicParam>; N] {
    names.map(|name| plugin.param(name).expect("plugin param").value.clone())
}

fn plugin_label(kind: PluginKind) -> &'static str {
    match kind {
        PluginKind::Synth(_) => "synth",
        PluginKind::Filter(FilterMode::LowPass) => "low-pass",
        PluginKind::Filter(FilterMode::HighPass) => "high-pass",
        PluginKind::Filter(FilterMode::BandPass) => "band-pass",
        PluginKind::Filter(FilterMode::Notch) => "notch",
        PluginKind::Filter(FilterMode::LowShelf) => "low shelf",
        PluginKind::Filter(FilterMode::HighShelf) => "high shelf",
        PluginKind::Filter(FilterMode::Peak) => "peak",
        PluginKind::Echo { .. } => "echo",
        PluginKind::Latency { .. } => "latency",
        PluginKind::Analyzer => "analyzer",
        PluginKind::Utility => "utility",
        PluginKind::Compressor => "compressor",
        PluginKind::Limiter { .. } => "limiter",
        PluginKind::Eq => "eq",
        PluginKind::Reverb => "reverb",
        PluginKind::Saturation(_) => "saturation",
        PluginKind::Gate => "gate",
    }
}
