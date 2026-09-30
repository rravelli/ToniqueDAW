//! A small polyphonic subtractive synth, as the built-in instrument.

use std::sync::Arc;

use crate::graph::{Node, NodeIdentity, NodeProperties, ProcessContext, StateTransfer};
use crate::midi::MidiMessage;
use crate::param::{AtomicParam, Smoother};

const VOICES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy, Debug)]
struct Voice {
    note: u8,
    stage: Stage,
    env: f32,
    velocity: f32,
    phase: f32,
    inc: f32,
    age: u64,
}

impl Voice {
    const IDLE: Voice = Voice {
        note: 0,
        stage: Stage::Off,
        env: 0.0,
        velocity: 0.0,
        phase: 0.0,
        inc: 0.0,
        age: 0,
    };
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope {
    pub attack_s: f32,
    pub decay_s: f32,
    pub sustain: f32,
    pub release_s: f32,
}

impl Default for Envelope {
    fn default() -> Self {
        Self {
            attack_s: 0.005,
            decay_s: 0.2,
            sustain: 0.6,
            release_s: 0.25,
        }
    }
}

/// PolyBLEP sawtooth voices with an ADSR, rendered sample-accurately
/// between MIDI events. Oldest voice is stolen when all are busy.
pub struct SynthNode {
    voices: [Voice; VOICES],
    env: Envelope,
    gain: Arc<AtomicParam>,
    gain_s: Smoother,
    sample_rate: f32,
    clock: u64,
    identity: Option<NodeIdentity>,
    // Per-sample envelope coefficients, computed in prepare.
    attack_step: f32,
    decay_coef: f32,
    release_coef: f32,
}

impl SynthNode {
    pub fn new(env: Envelope, gain: Arc<AtomicParam>) -> Self {
        Self {
            voices: [Voice::IDLE; VOICES],
            env,
            gain_s: Smoother::new(gain.get()),
            gain,
            sample_rate: 48000.0,
            clock: 0,
            identity: None,
            attack_step: 1.0,
            decay_coef: 0.0,
            release_coef: 0.0,
        }
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }

    /// Number of voices currently sounding.
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.stage != Stage::Off).count()
    }

    fn note_on(&mut self, note: u8, velocity: u8) {
        self.clock += 1;
        let idx = self
            .voices
            .iter()
            .position(|v| v.stage == Stage::Off)
            .unwrap_or_else(|| (0..VOICES).min_by_key(|&i| self.voices[i].age).unwrap());
        let freq = 440.0 * 2f32.powf((note as f32 - 69.0) / 12.0);
        self.voices[idx] = Voice {
            note,
            stage: Stage::Attack,
            env: self.voices[idx].env, // restart from current level: no click on steal
            velocity: velocity as f32 / 127.0,
            phase: 0.0,
            inc: freq / self.sample_rate,
            age: self.clock,
        };
    }

    fn note_off(&mut self, note: u8) {
        for v in self
            .voices
            .iter_mut()
            .filter(|v| v.note == note && v.stage != Stage::Off)
        {
            v.stage = Stage::Release;
        }
    }

    fn handle(&mut self, msg: MidiMessage) {
        match msg {
            MidiMessage::NoteOn { note, velocity, .. } => self.note_on(note, velocity),
            MidiMessage::NoteOff { note, .. } => self.note_off(note),
            MidiMessage::AllNotesOff { .. } => self
                .voices
                .iter_mut()
                .filter(|v| v.stage != Stage::Off)
                .for_each(|v| v.stage = Stage::Release),
            _ => {}
        }
    }

    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        for (l, r) in l.iter_mut().zip(r.iter_mut()) {
            let g = self.gain_s.next();
            let mut acc = 0.0;
            for v in self.voices.iter_mut().filter(|v| v.stage != Stage::Off) {
                match v.stage {
                    Stage::Attack => {
                        v.env += self.attack_step;
                        if v.env >= 1.0 {
                            v.env = 1.0;
                            v.stage = Stage::Decay;
                        }
                    }
                    Stage::Decay => {
                        v.env = self.env.sustain + (v.env - self.env.sustain) * self.decay_coef;
                        if (v.env - self.env.sustain).abs() < 1e-4 {
                            v.stage = Stage::Sustain;
                        }
                    }
                    Stage::Sustain => v.env = self.env.sustain,
                    Stage::Release => {
                        v.env *= self.release_coef;
                        if v.env < 1e-4 {
                            v.env = 0.0;
                            v.stage = Stage::Off;
                        }
                    }
                    Stage::Off => {}
                }
                acc += poly_blep_saw(v.phase, v.inc) * v.env * v.velocity;
                v.phase += v.inc;
                if v.phase >= 1.0 {
                    v.phase -= 1.0;
                }
            }
            let s = acc * g * 0.2;
            *l += s;
            *r += s;
        }
    }
}

#[inline]
fn poly_blep_saw(phase: f32, inc: f32) -> f32 {
    let mut y = 2.0 * phase - 1.0;
    if phase < inc {
        let t = phase / inc;
        y -= t + t - t * t - 1.0;
    } else if phase > 1.0 - inc {
        let t = (phase - 1.0) / inc;
        y -= t * t + t + t + 1.0;
    }
    y
}

impl Node for SynthNode {
    fn properties(&self) -> NodeProperties {
        let p = NodeProperties::audio(2);
        match self.identity {
            Some(id) => p.with_identity(id),
            None => p,
        }
    }

    fn prepare(&mut self, sample_rate: f64, _max_block: usize) {
        self.sample_rate = sample_rate as f32;
        let sr = self.sample_rate;
        self.attack_step = 1.0 / (self.env.attack_s * sr).max(1.0);
        // Exponential segments reaching ~-80 dB in the given time.
        self.decay_coef = (-9.2 / (self.env.decay_s * sr).max(1.0)).exp();
        self.release_coef = (-9.2 / (self.env.release_s * sr).max(1.0)).exp();
        self.gain_s.set_sample_rate(sample_rate);
        self.gain_s.snap(&self.gain);
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        self.gain_s.retarget(&self.gain);
        let len = ctx.block_len;
        let mut pos = 0usize;
        // Inputs are usually a single MIDI source; handle each input's
        // events in order, splitting the render at every event.
        for i in 0..ctx.num_inputs() {
            let events = ctx.midi_input(i);
            for e in events {
                let at = (e.offset as usize).min(len);
                if at > pos {
                    let (l, r) = ctx.audio_out.channel_pair_mut(0, 1);
                    self.render(&mut l[pos..at], &mut r[pos..at]);
                    pos = at;
                }
                self.handle(e.message);
            }
        }
        let (l, r) = ctx.audio_out.channel_pair_mut(0, 1);
        self.render(&mut l[pos..], &mut r[pos..]);
    }

    fn tail_samples(&self) -> usize {
        (self.env.release_s * self.sample_rate) as usize
    }

    /// Carry sounding voices over; the new envelope settings apply to them.
    fn take_state_from(&mut self, previous: &mut dyn Node) -> StateTransfer {
        if let Some(prev) = previous.as_any_mut().and_then(|a| a.downcast_mut::<Self>()) {
            self.voices = prev.voices;
            self.clock = prev.clock;
            self.gain_s = prev.gain_s.clone();
        }
        StateTransfer::KeepNew
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}
