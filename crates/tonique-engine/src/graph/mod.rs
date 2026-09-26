//! The processing graph: nodes, descriptions, compilation and execution.
//!
//! A [`GraphDescription`] is an ordinary owned DAG built off the RT thread.
//! [`compile`] turns it into a [`CompiledGraph`]: a flat, topologically
//! ordered schedule with a static buffer plan, which the RT thread runs
//! without further allocation or decision making.

mod compile;
mod compiled;
mod desc;
mod node;
pub mod scheduler;

pub use compile::{CompileError, CompileOptions, CompileStats, compile};
pub use compiled::CompiledGraph;
pub use desc::{GraphDescription, NodeId};
pub use node::{BlockInfo, ContentId, Node, NodeIdentity, NodeMessage, NodeProperties, ProcessContext, StateTransfer, hash_of};
