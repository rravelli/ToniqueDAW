use super::node::Node;

/// Handle to a node inside a [`GraphDescription`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub(crate) usize);

pub(crate) struct NodeSpec {
    pub node: Box<dyn Node>,
    /// Signal inputs (audio + MIDI of each is visible to the node).
    pub inputs: Vec<NodeId>,
    /// Ordering-only dependencies: run after these, but don't read them
    /// (e.g. an automation writer must run before the node it controls).
    pub after: Vec<NodeId>,
    /// Human-readable purpose, for inspection (see [`super::GraphTopology`]).
    pub label: Option<String>,
    /// Caller-defined ID of what this node belongs to (e.g. a track).
    pub owner: Option<u64>,
}

/// Off-thread, owned description of a processing graph.
#[derive(Default)]
pub struct GraphDescription {
    pub(crate) nodes: Vec<NodeSpec>,
    pub(crate) output: Option<NodeId>,
}

impl GraphDescription {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, node: impl Node + 'static, inputs: &[NodeId]) -> NodeId {
        self.add_boxed(Box::new(node), inputs)
    }

    pub fn add_boxed(&mut self, node: Box<dyn Node>, inputs: &[NodeId]) -> NodeId {
        self.nodes.push(NodeSpec {
            node,
            inputs: inputs.to_vec(),
            after: Vec::new(),
            label: None,
            owner: None,
        });
        NodeId(self.nodes.len() - 1)
    }

    /// Describe what `node` is for, e.g. `"Drums · fader"`. Only used for
    /// inspection; it doesn't affect processing.
    pub fn set_label(&mut self, node: NodeId, label: impl Into<String>) {
        self.nodes[node.0].label = Some(label.into());
    }

    /// Record what `node` belongs to, as an ID meaningful to the caller
    /// (e.g. a track's). Only used for inspection.
    pub fn set_owner(&mut self, node: NodeId, owner: u64) {
        self.nodes[node.0].owner = Some(owner);
    }

    /// Add a signal connection `from -> to` (may create a cycle, which
    /// compilation will reject).
    pub fn connect(&mut self, from: NodeId, to: NodeId) {
        self.nodes[to.0].inputs.push(from);
    }

    /// `node` must run after `before` in every block.
    pub fn add_order_dependency(&mut self, node: NodeId, before: NodeId) {
        self.nodes[node.0].after.push(before);
    }

    pub fn set_output(&mut self, node: NodeId) {
        self.output = Some(node);
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
