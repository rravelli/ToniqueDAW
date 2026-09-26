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

use super::{Bus, BusId, Channel, ClipContent, Edit, Output, Parameter, PluginKind, Track};
use crate::automation::AutomationCurve;
use crate::graph::{GraphDescription, NodeId, NodeIdentity};
use crate::nodes::{AudioClipNode, AutomationNode, ClipPlacement, DelayNode, EchoNode, FilterNode, MetronomeNode, MidiClipNode, SumNode, SynthNode, TimelineNote, VolumePanNode};
use crate::param::ParamId;
use crate::time::{BeatPos, SamplePos};

/// Identity of the automation writer for a parameter; used to address it
/// with [`crate::engine::Command::SendToNode`].
pub fn automation_identity(param: ParamId) -> NodeIdentity {
    NodeIdentity::of(&("automation", param.0))
}

pub fn build_graph(edit: &Edit, sample_rate: f64) -> GraphDescription {
    let mut b = Builder { edit, sr: sample_rate, d: GraphDescription::new() };
    b.build()
}

/// Curve for a parameter, converted from beats through the tempo map.
pub fn automation_curve(edit: &Edit, param: &Parameter, sample_rate: f64) -> Arc<AutomationCurve> {
    Arc::new(AutomationCurve::from_beats(&param.automation, &edit.tempo, sample_rate))
}

struct Builder<'a> {
    edit: &'a Edit,
    sr: f64,
    d: GraphDescription,
}

impl Builder<'_> {
    fn build(&mut self) -> GraphDescription {
        let master_in = self.d.add(SumNode::new(2), &[]);
        let bus_in: HashMap<BusId, NodeId> = self.edit.buses.iter().map(|b| (b.id, self.d.add(SumNode::new(2), &[]))).collect();
        let route = |o: Output| match o {
            Output::Master => master_in,
            Output::Bus(id) => bus_in.get(&id).copied().unwrap_or(master_in),
        };

        for track in &self.edit.tracks {
            let out = self.track(track);
            self.d.connect(out, route(track.output));
            for send in &track.sends {
                if let Some(&target) = bus_in.get(&send.bus) {
                    let id = NodeIdentity::of(&("send", track.id.0, send.level.id.0));
                    let node = VolumePanNode::new(send.level.value.clone(), send.pan.clone()).with_identity(id);
                    let s = self.d.add(node, &[out]);
                    self.automate(&send.level, s);
                    self.d.connect(s, target);
                }
            }
        }
        for bus in &self.edit.buses {
            let out = self.bus(bus, bus_in[&bus.id]);
            self.d.connect(out, route(bus.output));
        }
        let master = self.channel(&self.edit.master, master_in, NodeIdentity::of(&"master"));
        // The click bypasses the master fader and meter.
        let click = MetronomeNode::new(self.edit.tempo.clone(), self.edit.metronome.clone()).with_identity(NodeIdentity::of(&"metronome"));
        let click = self.d.add(click, &[]);
        let out = self.d.add(SumNode::new(2), &[master, click]);
        self.d.set_output(out);
        std::mem::take(&mut self.d)
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
                ClipContent::Audio { source, source_offset_s, gain } => {
                    // Not loaded yet: silent until the source is set.
                    let Some(source) = self.edit.source(*source) else { continue };
                    let place = ClipPlacement {
                        start,
                        length: end - start,
                        source_offset: (source_offset_s * self.sr).round() as SamplePos,
                        fade_in: (clip.fade_in_s * self.sr) as SamplePos,
                        fade_out: (clip.fade_out_s * self.sr) as SamplePos,
                    };
                    // Stateless: no identity needed, and dedupable by content.
                    self.d.add(AudioClipNode::new(source.clone(), place, *gain), &[])
                }
                ClipContent::Midi { notes, channel } => {
                    let tl: Vec<TimelineNote> = notes
                        .iter()
                        .map(|n| {
                            let s = self.samples(clip.start.0 + n.start);
                            TimelineNote { start: s, length: self.samples(clip.start.0 + n.start + n.length) - s, note: n.pitch, velocity: n.velocity }
                        })
                        .collect();
                    let node = MidiClipNode::new(&tl, *channel, start, end).with_identity(NodeIdentity::of(&("clip", clip.id.0)));
                    self.d.add(node, &[])
                }
            };
            sources.push(node);
        }
        let input = self.d.add(SumNode::new(2), &sources);
        self.channel(&track.channel, input, NodeIdentity::of(&("track", track.id.0)))
    }

    fn bus(&mut self, bus: &Bus, input: NodeId) -> NodeId {
        self.channel(&bus.channel, input, NodeIdentity::of(&("bus", bus.id.0)))
    }

    /// Plugin chain + fader for a track, bus or the master.
    fn channel(&mut self, ch: &Channel, input: NodeId, fader_identity: NodeIdentity) -> NodeId {
        let mut prev = input;
        for plugin in ch.plugins.iter().filter(|p| !p.bypassed) {
            let id = NodeIdentity::of(&("plugin", plugin.id.0));
            let param = |name| plugin.param(name).expect("plugin param").value.clone();
            prev = match plugin.kind {
                PluginKind::Synth(env) => self.d.add(SynthNode::new(env, param("gain")).with_identity(id), &[prev]),
                PluginKind::Filter(mode) => self.d.add(FilterNode::new(mode, param("cutoff"), param("q")).with_identity(id), &[prev]),
                PluginKind::Echo { time_s } => self.d.add(EchoNode::new(time_s, param("feedback"), param("mix")).with_identity(id), &[prev]),
                PluginKind::Latency { samples } => self.d.add(DelayNode::reporting(samples, 2), &[prev]),
            };
            for p in &plugin.params {
                self.automate(p, prev);
            }
        }
        let fader = VolumePanNode::new(ch.volume.value.clone(), ch.pan.value.clone())
            .with_gain(ch.mute_gain.clone())
            .with_meter(ch.meter.clone())
            .with_identity(fader_identity);
        let fader = self.d.add(fader, &[prev]);
        self.automate(&ch.volume, fader);
        self.automate(&ch.pan, fader);
        fader
    }

    /// Add an automation writer for `param` that runs before `owner`.
    fn automate(&mut self, param: &Parameter, owner: NodeId) {
        if param.automation.is_empty() {
            return;
        }
        let curve = automation_curve(self.edit, param, self.sr);
        let node = AutomationNode::new(curve, param.value.clone(), automation_identity(param.id));
        let a = self.d.add(node, &[]);
        self.d.add_order_dependency(owner, a);
    }
}
