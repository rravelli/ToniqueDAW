use std::sync::Arc;

use crate::automation::AutomationCurve;
use crate::graph::{Node, NodeIdentity, NodeMessage, NodeProperties, ProcessContext, StateTransfer};
use crate::param::AtomicParam;

/// Drives an [`AtomicParam`] from an automation curve while the transport
/// plays. Once per block it writes the curve value at the block's end with
/// a ramp exactly one block long, so the target's smoother reproduces the
/// curve piecewise-linearly at block resolution — automation *is* the ramp.
///
/// The compiler schedules it before the node owning the parameter via an
/// order-only dependency. Produces no audio.
pub struct AutomationNode {
    curve: Arc<AutomationCurve>,
    target: Arc<AtomicParam>,
    identity: NodeIdentity,
}

impl AutomationNode {
    pub fn new(curve: Arc<AutomationCurve>, target: Arc<AtomicParam>, identity: NodeIdentity) -> Self {
        Self { curve, target, identity }
    }
}

impl Node for AutomationNode {
    fn properties(&self) -> NodeProperties {
        NodeProperties::audio(0).with_identity(self.identity)
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        if !ctx.playing || self.curve.is_empty() {
            return;
        }
        let end = ctx.timeline_pos + ctx.block_len as i64;
        let ramp_ms = if ctx.jumped { 0.0 } else { (ctx.block_len as f64 / ctx.sample_rate * 1000.0) as f32 };
        let value = if ctx.jumped { self.curve.value_at(ctx.timeline_pos) } else { self.curve.value_at(end) };
        self.target.set(value, ramp_ms);
    }

    /// Swap in a new curve; the old one leaves in `msg` and is retired
    /// off-thread.
    fn receive(&mut self, msg: &mut NodeMessage) {
        if let NodeMessage::Curve(c) = msg {
            std::mem::swap(&mut self.curve, c);
        }
    }

    /// Stateless: the freshly built curve wins.
    fn take_state_from(&mut self, _previous: &mut dyn Node) -> StateTransfer {
        StateTransfer::KeepNew
    }
}
