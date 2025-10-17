pub mod audio;
pub mod bus;
pub mod midi;

use fundsp::{
    MAX_BUFFER_SIZE,
    hacker::{AudioUnit, BufferArray, Fade, NetBackend, pass},
    hacker32::U2,
    net::{Net, NodeId},
};

use std::collections::HashMap;

use crate::{
    audio::{
        clip::ClipBackend,
        track::{audio::AudioTrackData, bus::BusTrackData, midi::MidiTrackData},
    },
    metrics::AudioMetrics,
};

trait Processor {
    fn process(&mut self, pos: usize, num_frames: usize, sample_rate: usize, mix: &mut Vec<f32>);
}

#[derive(Clone)]
pub enum TrackKind {
    Audio(AudioTrackData),
    Midi(MidiTrackData),
    Bus(BusTrackData),
}

#[derive(Clone)]
/// Track struct for the audio threads. Process each clips and effects for that track.
pub struct TrackBackend {
    pub id: String,
    pub volume: f32,
    pub kind: TrackKind,

    pub muted: bool,
    pub net: Net,

    // Effects related
    backend: NetBackend,
    id_hash: HashMap<String, NodeId>,
    units: HashMap<NodeId, Box<dyn AudioUnit>>,
    node_order: Vec<NodeId>,

    pub metrics: AudioMetrics,
    pub mix: Vec<f32>,
}

impl TrackBackend {
    pub fn new(id: String, volume: f32, kind: TrackKind) -> Self {
        let mut net = Net::new(2, 2);
        // init network
        net.pass_through(0, 0);
        net.pass_through(1, 1);
        // create backend
        let backend = net.backend();

        TrackBackend {
            id,
            volume,
            kind,

            muted: false,
            backend,
            net,
            id_hash: HashMap::new(),
            units: HashMap::new(),
            node_order: Vec::new(),
            metrics: AudioMetrics::new(),
            mix: Vec::new(),
        }
    }

    pub fn process(&mut self, pos: usize, num_frames: usize, sample_rate: usize) {
        // Reset buffers
        self.mix.fill(0.);
        self.mix.resize(num_frames * 2, 0.);
        self.metrics.reset();

        // Render all clips into self.mix
        match &mut self.kind {
            TrackKind::Audio(audio_track_data) => {
                audio_track_data.process(pos, num_frames, sample_rate, &mut self.mix)
            }
            TrackKind::Midi(midi_track_data) => todo!(),
            TrackKind::Bus(bus_track_data) => {
                bus_track_data.process(pos, num_frames, sample_rate, &mut self.mix)
            }
        }

        if self.net.size() > 0 {
            self.process_effects();
        }
        // Update volume
        for (i, s) in self.mix.iter_mut().enumerate() {
            *s *= self.volume;
            self.metrics.add_sample(*s, (i % 2 == 0).into());
        }
    }

    fn process_effects(&mut self) {
        let mut input = BufferArray::<U2>::new();
        let mut output = BufferArray::<U2>::new();

        for chunk in self.mix.chunks_mut(2 * MAX_BUFFER_SIZE) {
            let size = chunk.len() / 2;

            // Fill input (deinterleave)
            for i in 0..size {
                input.set_f32(0, i, chunk[2 * i]);
                input.set_f32(1, i, chunk[2 * i + 1]);
            }

            // Process effects
            self.backend
                .process(size, &input.buffer_ref(), &mut output.buffer_mut());

            // Write back (re-interleave)
            for i in 0..size {
                chunk[2 * i] = output.at_f32(0, i);
                chunk[2 * i + 1] = output.at_f32(1, i);
            }
        }
    }

    pub fn disabled(&self, solo_tracks: &Vec<String>) -> bool {
        (self.muted && !solo_tracks.contains(&self.id))
            || (!solo_tracks.is_empty() && !solo_tracks.contains(&self.id))
    }

    pub fn remove_clip(&mut self, id: String) -> Option<ClipBackend> {
        if let TrackKind::Audio(data) = &mut self.kind
            && let Some(i) = data.clips.iter().position(|clip| clip.id == id)
        {
            return Some(data.clips.remove(i));
        }
        None
    }

    pub fn add_node(&mut self, id: String, node: Box<dyn AudioUnit>, index: usize) {
        let node_id = self.net.push(node.clone());

        if index > 0 {
            let id1 = self.node_order[index - 1];
            self.net.connect(id1, 0, node_id, 0);
            self.net.connect(id1, 1, node_id, 1);
        } else {
            self.net.connect_input(0, node_id, 0);
            self.net.connect_input(1, node_id, 1);
        }

        if index < self.node_order.len() {
            let id2 = self.node_order[index];
            self.net.connect(node_id, 0, id2, 0);
            self.net.connect(node_id, 1, id2, 1);
        } else {
            self.net.connect_output(node_id, 0, 0);
            self.net.connect_output(node_id, 1, 1);
        }

        self.id_hash.insert(id.clone(), node_id.clone());
        self.units.insert(node_id.clone(), node.clone());
        self.node_order.insert(index, node_id);
        self.net.commit();
    }

    pub fn remove_node(&mut self, id: String) {
        if let Some(node_id) = self.id_hash.get(&id) {
            self.net.remove_link(*node_id);
            self.net.commit();
            self.units.remove(&node_id);
            self.node_order.retain(|n| *node_id != *n);
        }
        // make sure to remove id to avoid a deadlock
        self.id_hash.remove(&id);
    }

    pub fn set_node_enabled(&mut self, id: String, enabled: bool) {
        if let Some(node_id) = self.id_hash.get(&id) {
            if enabled && let Some(unit) = self.units.get(&(*node_id)) {
                self.net
                    .crossfade(*node_id, Fade::Smooth, 0.01, unit.clone());
            } else {
                self.net
                    .crossfade(*node_id, Fade::Smooth, 0.01, Box::new(pass() | pass()));
            }
            self.net.commit();
        }
    }

    pub fn duplicate(&self, new_id: &String, clip_map: HashMap<String, String>) -> Self {
        let mut clone = self.clone();
        clone.id = new_id.clone();

        match &mut clone.kind {
            TrackKind::Audio(audio_track_data) => {
                for clip in audio_track_data.clips.iter_mut() {
                    if let Some(new_clip_id) = clip_map.get(&clip.id) {
                        clip.id = new_clip_id.clone();
                    };
                }
            }
            TrackKind::Midi(midi_track_data) => {}
            TrackKind::Bus(bus_track_data) => {}
        }
        clone
    }
}
