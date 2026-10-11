//! Records a plugin's input and output for its editor.

use std::any::Any;
use std::sync::Arc;

use crate::graph::{Node, NodeMessage, NodeProperties, ProcessContext, StateTransfer};
use crate::meter::PluginTap;

/// Runs `inner`, recording what goes in and out of it into a
/// [`PluginTap`] while that's watched. Otherwise transparent: same
/// properties, and state carries over rebuilds as `inner`'s would.
pub struct TappedNode<N> {
    inner: N,
    tap: Arc<PluginTap>,
}

impl<N: Node + 'static> TappedNode<N> {
    pub fn new(inner: N, tap: Arc<PluginTap>) -> Self {
        Self { inner, tap }
    }
}

impl<N: Node + 'static> Node for TappedNode<N> {
    fn properties(&self) -> NodeProperties {
        self.inner.properties()
    }

    fn prepare(&mut self, sample_rate: f64, max_block: usize) {
        self.inner.prepare(sample_rate, max_block);
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        let [input, output] = self.tap.take_block(ctx.block_len);
        // Plugins in a chain have one input: the previous plugin's output.
        if input && ctx.num_inputs() > 0 {
            self.tap.input.record(&ctx.input(0));
        }
        self.inner.process(ctx);
        if output {
            self.tap.output.record(&ctx.audio_out.as_block());
        }
    }

    fn tail_samples(&self) -> usize {
        self.inner.tail_samples()
    }

    fn receive(&mut self, msg: &mut NodeMessage) {
        self.inner.receive(msg);
    }

    fn take_state_from(&mut self, previous: &mut dyn Node) -> StateTransfer {
        match previous.as_any_mut().and_then(|a| a.downcast_mut::<Self>()) {
            Some(prev) => self.inner.take_state_from(&mut prev.inner),
            None => self.inner.take_state_from(previous),
        }
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn Any> {
        Some(self)
    }

    fn name(&self) -> &'static str {
        self.inner.name()
    }
}
