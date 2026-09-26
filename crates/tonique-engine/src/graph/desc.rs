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
        self.nodes.push(NodeSpec { node, inputs: inputs.to_vec(), after: Vec::new() });
        NodeId(self.nodes.len() - 1)
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
