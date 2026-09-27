//! Tonique: a graph-based, real-time safe audio engine for DAWs, in the
//! spirit of Tracktion Engine.
//!
//! Layers, bottom to top:
//! - [`graph`]: the `Node` trait, graph description/compilation, execution
//!   (sequential and parallel);
//! - [`nodes`]: built-in nodes (clips, synth, effects, mixing, automation);
//! - [`engine`]: the RT processor, transport, command ring, graph hand-off
//!   and deferred destruction;
//! - [`edit`]: the document model (tracks, clips, plugins, automation) with
//!   undo, and the builder that turns an edit into a graph.

pub mod audio;
pub mod automation;
pub mod edit;
pub mod engine;
pub mod graph;
pub mod meter;
pub mod midi;
pub mod nodes;
pub mod param;
pub mod peaks;
pub mod preview;
pub mod resample;
pub mod rt;
pub mod sample;
pub mod time;

#[cfg(feature = "device")]
pub mod device;
