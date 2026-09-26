use std::any::Any;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use crate::audio::{AudioBlock, AudioBlockMut};
use crate::automation::AutomationCurve;
use crate::midi::MidiEventList;
use crate::time::SamplePos;

/// Deterministic 64-bit hash, used for content IDs and identities.
pub fn hash_of<T: Hash + ?Sized>(value: &T) -> u64 {
    let mut h = DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

/// Hash of *what a node computes*: (type, parameters). The compiler folds
/// in the content IDs of the node's inputs, so two nodes with equal final
/// IDs provably produce identical output and one of them can be dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContentId(pub u64);

impl ContentId {
    pub fn of<T: Hash + ?Sized>(value: &T) -> Self {
        Self(hash_of(value))
    }
}

/// Stable identity of a stateful instance across graph rebuilds (a plugin
/// instance ID, a clip ID, ...). Never derived from graph position. When a
/// new graph replaces an old one, instances with matching identities are
/// moved across so reverb tails, filter memory etc. survive edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeIdentity(pub u64);

impl NodeIdentity {
    pub fn of<T: Hash + ?Sized>(value: &T) -> Self {
        Self(hash_of(value))
    }
}

/// Static properties, queried once at compile time (off the RT thread).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeProperties {
    /// Audio output channels (0 for control-only nodes such as automation).
    pub channels: usize,
    /// Whether the node produces MIDI output.
    pub has_midi: bool,
    /// Processing latency; the compiler inserts compensation delays on
    /// sibling branches so everything lines up at summing points.
    pub latency_samples: usize,
    /// `None` means "never deduplicate" (e.g. anything with unique state).
    pub content_id: Option<ContentId>,
    /// `None` means "always a fresh instance".
    pub identity: Option<NodeIdentity>,
}

impl NodeProperties {
    pub fn audio(channels: usize) -> Self {
        Self { channels, has_midi: false, latency_samples: 0, content_id: None, identity: None }
    }

    pub fn midi() -> Self {
        Self { has_midi: true, ..Self::audio(0) }
    }

    pub fn with_midi(mut self) -> Self {
        self.has_midi = true;
        self
    }

    pub fn with_latency(mut self, samples: usize) -> Self {
        self.latency_samples = samples;
        self
    }

    pub fn with_content(mut self, id: ContentId) -> Self {
        self.content_id = Some(id);
        self
    }

    pub fn with_identity(mut self, id: NodeIdentity) -> Self {
        self.identity = Some(id);
        self
    }
}

/// Per-block timing handed to every node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockInfo {
    pub block_len: usize,
    pub sample_rate: f64,
    /// Sample-accurate position of this block on the project timeline.
    pub timeline_pos: SamplePos,
    pub playing: bool,
    /// Set on the first block after start, seek or loop wrap-around, so
    /// nodes can flush state that assumed continuous playback.
    pub jumped: bool,
}

/// Outcome of [`Node::take_state_from`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateTransfer {
    /// Use the previous instance as-is (state *and* configuration). Right
    /// for nodes configured only through shared params.
    KeepPrevious,
    /// Keep the fresh instance: it copied whatever state it needed.
    KeepNew,
}

/// Messages delivered to a node on the RT thread (see [`Node::receive`]).
/// A node takes what it needs by swapping; whatever is left in the message
/// afterwards (e.g. the old curve) is retired off the RT thread.
#[derive(Debug, Default)]
pub enum NodeMessage {
    #[default]
    None,
    Curve(Arc<AutomationCurve>),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct NodeIo {
    pub audio: *mut f32,
    pub channels: usize,
    pub stride: usize,
    pub midi: *mut MidiEventList,
}

static EMPTY_MIDI: MidiEventList = MidiEventList::empty();

pub struct ProcessContext<'a> {
    pub(crate) input_indices: &'a [u32],
    pub(crate) io: &'a [NodeIo],
    pub audio_out: AudioBlockMut<'a>,
    pub midi_out: &'a mut MidiEventList,
    pub block_len: usize,
    pub sample_rate: f64,
    pub timeline_pos: SamplePos,
    pub playing: bool,
    pub jumped: bool,
}

impl<'a> ProcessContext<'a> {
    pub fn num_inputs(&self) -> usize {
        self.input_indices.len()
    }

    pub fn input(&self, i: usize) -> AudioBlock<'a> {
        let io = self.io[self.input_indices[i] as usize];
        // SAFETY: the scheduler guarantees every input has finished writing
        // and that its slot is not reassigned while this node runs.
        unsafe { AudioBlock::from_raw(io.audio, io.channels, io.stride, self.block_len) }
    }

    pub fn midi_input(&self, i: usize) -> &'a MidiEventList {
        let io = self.io[self.input_indices[i] as usize];
        // SAFETY: as for `input`; MIDI lists are per node and never shared for writing.
        if io.midi.is_null() { &EMPTY_MIDI } else { unsafe { &*io.midi } }
    }

    pub fn inputs(&self) -> impl Iterator<Item = AudioBlock<'a>> + '_ {
        (0..self.num_inputs()).map(|i| self.input(i))
    }

    pub fn midi_inputs(&self) -> impl Iterator<Item = &'a MidiEventList> + '_ {
        (0..self.num_inputs()).map(|i| self.midi_input(i))
    }

    /// Sum all audio inputs into the output.
    pub fn sum_inputs_to_output(&mut self) {
        for i in 0..self.num_inputs() {
            let block = self.input(i);
            self.audio_out.add_from(&block, 1.0);
        }
    }

    /// Merge all MIDI inputs into the MIDI output, sorted by offset.
    pub fn merge_midi_inputs_to_output(&mut self) {
        for i in 0..self.num_inputs() {
            let list = self.midi_input(i);
            self.midi_out.extend_from(list);
        }
        self.midi_out.sort();
    }

    pub fn block_info(&self) -> BlockInfo {
        BlockInfo {
            block_len: self.block_len,
            sample_rate: self.sample_rate,
            timeline_pos: self.timeline_pos,
            playing: self.playing,
            jumped: self.jumped,
        }
    }
}

/// A unit of processing in the graph.
///
/// Threading contract:
/// - `properties` and `prepare` run on the graph-building thread during
///   [`compile`](super::compile), before the node is ever visible to the RT
///   thread, so `prepare` may allocate (delay lines, voice pools, ...).
/// - `process` and `receive` run on the RT thread (or a worker helping it)
///   and MUST NOT allocate, lock, or block.
/// - A node is never processed concurrently with itself.
pub trait Node: Send {
    fn properties(&self) -> NodeProperties;

    fn prepare(&mut self, _sample_rate: f64, _max_block: usize) {}

    /// The output buffers are cleared before this is called.
    fn process(&mut self, ctx: &mut ProcessContext);

    /// Non-zero for reverb/delay tails: output may continue after input stops.
    fn tail_samples(&self) -> usize {
        0
    }

    /// RT-thread message delivery. Swap what you need out of `msg`.
    fn receive(&mut self, _msg: &mut NodeMessage) {}

    /// Called on the RT thread when this freshly compiled node replaces a
    /// previous node with the same [`NodeIdentity`]. Nodes whose
    /// configuration is baked in at construction (clip contents, envelope
    /// times, ...) should downcast `previous` (see [`Node::as_any_mut`]),
    /// copy or swap over their *state* only and return `KeepNew`.
    /// Must not allocate: swap buffers rather than cloning them.
    fn take_state_from(&mut self, _previous: &mut dyn Node) -> StateTransfer {
        StateTransfer::KeepPrevious
    }

    /// Enables downcasting in [`Node::take_state_from`]; implement as `Some(self)`.
    fn as_any_mut(&mut self) -> Option<&mut dyn Any> {
        None
    }

    fn name(&self) -> &'static str {
        let full = std::any::type_name::<Self>();
        full.rsplit("::").next().unwrap_or(full)
    }
}
