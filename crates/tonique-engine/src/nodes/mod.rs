//! Built-in processing nodes.

mod automation;
mod basic;
mod clip;
mod dynamics;
mod eq;
mod fx;
mod metronome;
mod reverb;
mod saturation;
mod synth;
mod tap;

pub use automation::AutomationNode;
pub use basic::{DelayNode, OscillatorNode, SumNode, ThroughNode, VolumePanNode, Waveform};
pub use clip::{
    AudioClipNode, ClipPlacement, ClipWarp, LoopPlacement, MidiClipNode, TimedMidi, TimelineNote,
    balance,
};
pub use dynamics::{CompressorNode, GATE_SILENT_DB, GateNode, LimiterNode, compressor_reduction};
pub use eq::{EQ_BANDS, EQ_GAIN_DB, EqBand, EqNode, eq_mode, eq_mode_value};
pub use fx::{EchoNode, FilterMode, FilterNode, UTILITY_SILENT_DB, UtilityNode};
pub use metronome::MetronomeNode;
pub use reverb::{REVERB_MAX_PREDELAY_MS, ReverbNode};
pub use saturation::{SaturationCurve, SaturationNode};
pub use synth::{Envelope, SynthNode};
pub use tap::TappedNode;
