//! Built-in processing nodes.

mod automation;
mod basic;
mod clip;
mod fx;
mod metronome;
mod synth;
mod tap;

pub use automation::AutomationNode;
pub use basic::{DelayNode, OscillatorNode, SumNode, ThroughNode, VolumePanNode, Waveform};
pub use clip::{AudioClipNode, ClipPlacement, MidiClipNode, TimedMidi, TimelineNote};
pub use fx::{EchoNode, FilterMode, FilterNode, UTILITY_SILENT_DB, UtilityNode};
pub use metronome::MetronomeNode;
pub use synth::{Envelope, SynthNode};
pub use tap::TappedNode;
