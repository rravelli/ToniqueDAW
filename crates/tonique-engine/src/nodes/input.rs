//! Brings an input device's audio into the graph.

use crate::graph::{ContentId, Node, NodeProperties, ProcessContext};
use crate::input::InputRoute;

/// Outputs channels of the input device, in stereo, silent while there's no
/// input or no such channel. Stateless, so two tracks listening to the same
/// channels share one.
pub struct InputNode {
    route: InputRoute,
}

impl InputNode {
    pub fn new(route: InputRoute) -> Self {
        Self { route }
    }
}

impl Node for InputNode {
    fn properties(&self) -> NodeProperties {
        NodeProperties::audio(2).with_content(ContentId::of(&("input", self.route)))
    }

    fn process(&mut self, ctx: &mut ProcessContext) {
        let (left, right) = match self.route {
            InputRoute::Mono(channel) => (channel, channel),
            InputRoute::Stereo(channel) => (channel, channel + 1),
        };
        for (out, channel) in [left, right].into_iter().enumerate() {
            if let Some(input) = ctx.device_input(channel) {
                ctx.audio_out.channel_mut(out).copy_from_slice(input);
            }
        }
    }
}
