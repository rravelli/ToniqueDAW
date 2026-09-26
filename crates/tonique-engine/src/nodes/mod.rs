//! Built-in processing nodes.

mod automation;
mod basic;
mod clip;
mod fx;
mod metronome;
mod synth;

pub use automation::AutomationNode;
pub use basic::{DelayNode, OscillatorNode, SumNode, VolumePanNode, Waveform};
pub use clip::{AudioClipNode, ClipPlacement, MidiClipNode, TimedMidi, TimelineNote};
pub use fx::{EchoNode, FilterMode, FilterNode};
pub use metronome::MetronomeNode;
pub use synth::{Envelope, SynthNode};
