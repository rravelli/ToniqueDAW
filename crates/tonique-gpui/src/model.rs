//! The project being edited, as a gpui model: every view holds the same
//! `Entity<Project>` and reads Tonique's `ProjectState` through it.

use std::ops::{Deref, DerefMut};

use gpui::{App, Entity, GpuiBorrow};

use crate::core::state::ProjectState;

pub struct Project {
    pub state: ProjectState,
}

impl Project {
    pub fn new(state: ProjectState) -> Self {
        Self { state }
    }
}

/// The project's state, borrowed for changes; its views hear of them when
/// it's let go.
pub struct StateMut<'app>(GpuiBorrow<'app, Project>);

impl Deref for StateMut<'_> {
    type Target = ProjectState;

    fn deref(&self) -> &ProjectState {
        &self.0.state
    }
}

impl DerefMut for StateMut<'_> {
    fn deref_mut(&mut self) -> &mut ProjectState {
        &mut self.0.state
    }
}

/// Reading and changing the project's state from a view holding it.
pub trait HoldsProject {
    fn project(&self) -> &Entity<Project>;

    fn state<'app>(&self, app: &'app App) -> &'app ProjectState {
        &self.project().read(app).state
    }

    fn state_mut<'app>(&self, app: &'app mut App) -> StateMut<'app> {
        StateMut(self.project().as_mut(app))
    }
}
