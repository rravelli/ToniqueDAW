use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crossbeam_queue::ArrayQueue;

use super::compile::{CompileOptions, CompileStats};
use super::node::{BlockInfo, Node, NodeIdentity, NodeIo, NodeMessage, NodeProperties, ProcessContext, StateTransfer};
use crate::audio::{AudioBlock, AudioBlockMut, AudioBuffer};
use crate::midi::MidiEventList;

pub(crate) struct CompiledNode {
    node: UnsafeCell<Box<dyn Node>>,
    name: &'static str,
    props: NodeProperties,
    inputs: Vec<u32>,
}

impl CompiledNode {
    pub(crate) fn new(node: Box<dyn Node>, name: &'static str, props: NodeProperties, inputs: Vec<usize>) -> Self {
        Self { node: UnsafeCell::new(node), name, props, inputs: inputs.into_iter().map(|i| i as u32).collect() }
    }
}

/// An executable graph: nodes in topological order, a fixed buffer plan and
/// precomputed dependency counts. Everything the RT thread needs is
/// allocated up front.
pub struct CompiledGraph {
    nodes: Vec<CompiledNode>,
    io: Vec<NodeIo>,
    dependents: Vec<Vec<u32>>,
    /// In-degree per node; copied into `remaining` at the top of each block.
    dep_template: Vec<u32>,
    pub(crate) remaining: Vec<AtomicU32>,
    pub(crate) done: AtomicUsize,
    pub(crate) ready: ArrayQueue<u32>,
    roots: Vec<u32>,
    identity_index: Vec<(NodeIdentity, u32)>,
    output: usize,
    // Owned storage behind the raw pointers in `io`. Never resized after
    // `build`, so the pointers stay valid for the graph's lifetime.
    _slots: Vec<AudioBuffer>,
    _midi: Vec<MidiEventList>,
    #[cfg_attr(not(test), allow(dead_code))]
    slot_of: Vec<Option<usize>>,
    opts: CompileOptions,
    stats: CompileStats,
}

// SAFETY: the raw pointers in `io` point into heap storage owned by this
// struct. Shared (`&self`) access from several threads happens only in the
// parallel scheduler, which guarantees (a) each node's `process` runs on one
// thread at a time, (b) a node runs only after all its predecessors finished
// (release/acquire through the dependency counters and ready queue), and
// (c) the buffer plan never lets a node write a slot that a concurrently
// runnable node reads.
unsafe impl Send for CompiledGraph {}
unsafe impl Sync for CompiledGraph {}

impl CompiledGraph {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build(
        nodes: Vec<CompiledNode>,
        dependents: Vec<Vec<usize>>,
        dep_template: Vec<u32>,
        slot_of: Vec<Option<usize>>,
        slot_count: usize,
        slot_channels: usize,
        identity_index: Vec<(NodeIdentity, u32)>,
        output: usize,
        opts: CompileOptions,
        stats: CompileStats,
    ) -> Self {
        let n = nodes.len();
        let mut slots: Vec<AudioBuffer> = (0..slot_count).map(|_| AudioBuffer::new(slot_channels, opts.max_block)).collect();
        let mut midi: Vec<MidiEventList> =
            nodes.iter().map(|c| if c.props.has_midi { MidiEventList::default() } else { MidiEventList::empty() }).collect();
        let io = (0..n)
            .map(|i| {
                let (audio, channels) = match slot_of[i] {
                    Some(s) => (slots[s].as_mut_ptr(), nodes[i].props.channels),
                    None => (std::ptr::null_mut(), 0),
                };
                NodeIo { audio, channels, stride: opts.max_block, midi: &mut midi[i] as *mut _ }
            })
            .collect();
        let roots = (0..n as u32).filter(|&i| dep_template[i as usize] == 0).collect();
        Self {
            io,
            dependents: dependents.into_iter().map(|d| d.into_iter().map(|x| x as u32).collect()).collect(),
            remaining: dep_template.iter().map(|&d| AtomicU32::new(d)).collect(),
            dep_template,
            done: AtomicUsize::new(0),
            ready: ArrayQueue::new(n.max(1)),
            roots,
            identity_index,
            output,
            _slots: slots,
            _midi: midi,
            slot_of,
            opts,
            stats,
            nodes,
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn stats(&self) -> CompileStats {
        self.stats
    }

    pub fn sample_rate(&self) -> f64 {
        self.opts.sample_rate
    }

    pub fn max_block(&self) -> usize {
        self.opts.max_block
    }

    pub fn output_channels(&self) -> usize {
        self.nodes[self.output].props.channels
    }

    /// Node names in schedule order (for debugging and tests).
    pub fn node_names(&self) -> Vec<&'static str> {
        self.nodes.iter().map(|n| n.name).collect()
    }

    pub(crate) fn roots(&self) -> &[u32] {
        &self.roots
    }

    pub(crate) fn dependents(&self, idx: usize) -> &[u32] {
        &self.dependents[idx]
    }

    /// Reset per-block scheduling counters (a cheap copy, no graph walk).
    pub(crate) fn reset_counters(&self) {
        for (r, &d) in self.remaining.iter().zip(&self.dep_template) {
            r.store(d, Ordering::Relaxed);
        }
        self.done.store(0, Ordering::Relaxed);
    }

    /// Run one node.
    ///
    /// # Safety
    /// No other thread may be processing `idx`, all of `idx`'s predecessors
    /// must have completed (with a happens-before edge to this call), and
    /// `info.block_len <= max_block`.
    pub(crate) unsafe fn process_node(&self, idx: usize, info: &BlockInfo) {
        let cn = &self.nodes[idx];
        let io = self.io[idx];
        let mut ctx = ProcessContext {
            input_indices: &cn.inputs,
            io: &self.io,
            audio_out: unsafe { AudioBlockMut::from_raw(io.audio, io.channels, io.stride, info.block_len) },
            midi_out: unsafe { &mut *io.midi },
            block_len: info.block_len,
            sample_rate: info.sample_rate,
            timeline_pos: info.timeline_pos,
            playing: info.playing,
            jumped: info.jumped,
        };
        ctx.audio_out.clear();
        ctx.midi_out.clear();
        let node = unsafe { &mut *cn.node.get() };
        node.process(&mut ctx);
    }

    /// Run every node in schedule order on the calling thread.
    pub fn process_sequential(&mut self, info: &BlockInfo) {
        assert!(info.block_len <= self.opts.max_block);
        for i in 0..self.nodes.len() {
            // SAFETY: `&mut self` gives exclusive access and nodes are stored
            // in topological order.
            unsafe { self.process_node(i, info) };
        }
    }

    /// The output node's audio for the last processed block.
    pub fn output(&self, len: usize) -> AudioBlock<'_> {
        let io = self.io[self.output];
        // SAFETY: only called when no block is being processed.
        unsafe { AudioBlock::from_raw(io.audio, io.channels, io.stride, len.min(self.opts.max_block)) }
    }

    /// Carry state over from `old` for every node whose identity matches:
    /// either by taking the old instance wholesale (it is swapped into this
    /// graph, and the fresh one retires with `old`), or by letting the fresh
    /// node copy state out of it (see [`Node::take_state_from`]).
    /// RT-safe: a merge-join over two sorted arrays plus pointer swaps.
    /// Returns how many nodes had state carried over.
    pub fn adopt_state_from(&mut self, old: &mut CompiledGraph) -> usize {
        let (mut i, mut j, mut moved) = (0, 0, 0);
        while i < self.identity_index.len() && j < old.identity_index.len() {
            let (a, ai) = self.identity_index[i];
            let (b, bi) = old.identity_index[j];
            if a < b {
                i += 1;
            } else if b < a {
                j += 1;
            } else {
                let new = &mut self.nodes[ai as usize];
                let prev = &mut old.nodes[bi as usize];
                let (new_node, prev_node) = (new.node.get_mut(), prev.node.get_mut());
                match new_node.take_state_from(prev_node.as_mut()) {
                    StateTransfer::KeepNew => moved += 1,
                    // Only swap if the compiled buffer layout still fits the instance.
                    StateTransfer::KeepPrevious
                        if new.props.channels == prev.props.channels
                            && new.props.has_midi == prev.props.has_midi
                            && new.props.latency_samples == prev.props.latency_samples =>
                    {
                        std::mem::swap(new_node, prev_node);
                        moved += 1;
                    }
                    StateTransfer::KeepPrevious => {}
                }
                i += 1;
                j += 1;
            }
        }
        moved
    }

    /// Deliver a message to the node with `identity`. Returns false if no
    /// such node exists (the message is left untouched).
    pub fn deliver(&mut self, identity: NodeIdentity, msg: &mut NodeMessage) -> bool {
        match self.identity_index.binary_search_by_key(&identity, |e| e.0) {
            Ok(k) => {
                let idx = self.identity_index[k].1 as usize;
                self.nodes[idx].node.get_mut().receive(msg);
                true
            }
            Err(_) => false,
        }
    }

    /// Longest tail among nodes (how long to keep rendering after the end).
    pub fn max_tail_samples(&mut self) -> usize {
        self.nodes.iter_mut().map(|n| n.node.get_mut().tail_samples()).max().unwrap_or(0)
    }

    #[cfg(test)]
    pub(crate) fn slot_assignment(&self) -> Vec<Option<usize>> {
        self.slot_of.clone()
    }

    #[cfg(test)]
    pub(crate) fn index_of_name_nth(&self, name: &str, nth: usize) -> usize {
        self.nodes.iter().enumerate().filter(|(_, n)| n.name == name).nth(nth).unwrap().0
    }
}
