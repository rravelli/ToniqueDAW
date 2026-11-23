//! The project being edited, as a gpui model: every view holds the same
//! `Entity<Project>` and reads Tonique's `ProjectState` through it.
//!
//! A view re-renders when an entity it read changes, so what changes
//! every frame while the project plays is told apart: the project is
//! notified when it's edited, or when an update changed it (a file loaded,
//! the audio lost); what plays, and the processor's load, have signals of
//! their own, which only the views showing them watch.

use std::{
    ops::{Deref, DerefMut},
    time::{Duration, Instant},
};

use gpui::{App, AppContext, Entity, GpuiBorrow};

use crate::core::state::{PlaybackState, ProjectState};

pub struct Project {
    pub state: ProjectState,
    /// Notified as the playhead, the preview and the meters move.
    playback: Entity<Signal>,
    /// Notified as the processor's load shown changes.
    load: Entity<Signal>,
    /// The load last shown, in whole percents.
    load_shown: i32,
    /// When the views last heard of files decoding.
    loading_told: Option<Instant>,
}

/// How often views hear of files decoding: their waveforms grow, their
/// tempo comes.
const LOADING_INTERVAL: Duration = Duration::from_millis(100);

/// Something changing that views can watch apart from the project.
pub struct Signal;

impl Project {
    pub fn new(state: ProjectState, context: &mut App) -> Self {
        Self {
            state,
            playback: context.new(|_| Signal),
            load: context.new(|_| Signal),
            load_shown: 0,
            loading_told: None,
        }
    }

    /// Whether something plays or moves: the playhead, the preview, the
    /// meters falling back after playback.
    pub fn is_playing(&self) -> bool {
        let state = &self.state;
        state.playback_state() == PlaybackState::Playing
            || state.preview_playback_state() == PlaybackState::Playing
            || state.metrics.tracks.values().any(crate::meter::is_live)
    }
}

/// Tells the views reading `signal` it changed: notified alone, it
/// would only re-render views watching it for their own sake.
fn tell(signal: &Entity<Signal>, context: &mut App) {
    signal.update(context, |_, context| context.notify());
}

/// Polls the engine and the file loaders, as each frame does in the egui
/// app; tells the views watching what changed. Returns whether something
/// plays, to tick again soon.
pub fn tick(project: &Entity<Project>, context: &mut App) -> bool {
    project.update(context, |project, context| {
        let preview = project.state.preview_playback_state();
        let was_playing = project.is_playing();
        let changed = crate::profile::time("state.update", || project.state.update());
        let now = Instant::now();
        let loading = project.state.sources_loading()
            && project
                .loading_told
                .is_none_or(|told| now - told >= LOADING_INTERVAL);
        if loading {
            project.loading_told = Some(now);
        }
        if changed || loading {
            context.notify();
        }
        let playing = project.is_playing();
        // Once more as it stops, for the views to show it stopped.
        if playing || was_playing || preview != project.state.preview_playback_state() {
            tell(&project.playback, context);
        }
        let load = (project.state.metrics.latency * 100.).round() as i32;
        if load != project.load_shown {
            project.load_shown = load;
            tell(&project.load, context);
        }
        playing
    })
}

/// Plays from the playhead, or stops.
pub fn toggle_play(state: &mut ProjectState) {
    if state.playback_state() == PlaybackState::Playing {
        state.stop();
    } else {
        state.play();
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

    /// Renders the view again as the playhead, the preview and the meters
    /// move: called from its render.
    fn watch_playback(&self, app: &App) {
        self.project().read(app).playback.read(app);
    }

    /// Renders the view again as the processor's load shown changes.
    fn watch_load(&self, app: &App) {
        self.project().read(app).load.read(app);
    }
}
