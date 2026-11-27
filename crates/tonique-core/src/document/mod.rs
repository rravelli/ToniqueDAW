//! The document as the UI sees it: views of the engine's edit (which
//! holds the document itself, with its commands and undo history) and
//! what's saved with it besides. Clips, tracks and effects as built from
//! the edit, with their rules; how the document looks (saved, not
//! undoable); the clip selections of its history; the project file.

pub mod clip;
pub mod clip_ops;
pub mod effect;
pub mod file;
pub mod history;
pub mod track;
pub mod view;
