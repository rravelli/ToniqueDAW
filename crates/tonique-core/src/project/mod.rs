//! [`ProjectState`]: the facade the UIs drive. It holds the engine's edit
//! session and the parts around it (the document's view and history, the
//! session, the transport, the preview, the monitor, the audio output),
//! and runs what involves several of them, one file per subject.

mod actions;
mod clipboard;
mod clips;
mod device;
mod effects;
mod export;
mod groups;
mod load;
mod mixer;
mod monitor;
mod preview;
mod selection;
#[cfg(test)]
mod tests;
mod tracks;
mod transport;
use crate::{
    config::settings::Settings,
    document::track::{TRACK_COLLAPSED_HEIGHT, TrackKind, TrackRow, TrackSoloState, TrackView},
    library::sources::SourceRegistry,
    session::grid::GridService,
    utils::color::Color,
};
use std::mem::take;

use crate::device::output::AudioOutput;
use crate::document::history::History;
use crate::document::view::DocumentView;
use crate::monitor::Monitor;
use crate::session::Session;
pub use crate::session::selection::SelectionBounds;
use crate::transport::Transport;
use crate::transport::preview::Preview;
pub use actions::ActionOutcome;
pub use export::{ExportSelection, ExportSource};
pub use groups::RowTarget;
use tonique_engine::{
    edit::{
        BusId, ChannelRef, ClipId, Edit, EditError, EditSession, TrackId,
        commands::{EditCommand, RenameBus, RenameTrack},
    },
    engine::Engine,
    time::BeatPos,
};

pub use crate::document::track::MASTER_TRACK_ID;

const DEFAULT_BPM: f64 = 120.;
/// A new track's name; `#` is its number.
pub const TRACK_NAME: &str = "# Audio Track";
/// Smallest height of an expanded row (its controls must fit).
pub const MIN_EXPANDED_HEIGHT: f32 = TRACK_COLLAPSED_HEIGHT + 25.;

#[derive(Clone, Debug)]
enum ProjectStatePendingAction {
    DeleteTrack { id: TrackId },
}

/// An audio clip's parameter: heard as it's set, like a fader's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipParam {
    /// Linear.
    Gain,
    /// Balance, -1 (left only) to 1 (right only).
    Pan,
}

/// What [`ProjectState::update`] changed.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Updated {
    /// The project, besides what plays: an action handled, a file loaded,
    /// the audio lost or back.
    pub project: bool,
    /// What plays: the playhead or the preview moved, or one stopped.
    pub playback: bool,
}

pub use crate::transport::PlaybackState;

/// The project as the UI sees it. The engine's [`EditSession`] owns the
/// document and its undo history; the parts around it keep their own
/// rules: how the document looks, the selections of its history, the
/// session, the transport, the preview, the monitor and the audio output.
/// This runs what needs several of them (deleting the selected clips
/// edits the document, the selection and the history; restarting the
/// audio stops, seeks and plays again).
pub struct ProjectState {
    /// The engine's document, its undo history, and the engine.
    edit_session: EditSession,
    sources: SourceRegistry,
    /// The audio device, and reopening it when lost.
    output: AudioOutput,
    /// Applied preferences; change them with `apply_settings`.
    settings: Settings,
    /// The playhead, the edit cursor, the loop and the metronome.
    transport: Transport,
    /// The browser's file preview.
    preview: Preview,
    /// The meters, and the graph view's measures.
    monitor: Monitor,

    /// How the document looks, saved with it but not undoable.
    view: DocumentView,
    /// What the user is working on, never saved: the selection, the
    /// clipboard, a trim under way.
    session: Session,
    /// The selections of the undo history, in step with the engine's.
    history: History,
    pending_actions: Vec<ProjectStatePendingAction>,
    batching: bool,

    pub grid: GridService,
    /// An undo step was opened for a clip edit heard as it's made (see
    /// [`Self::edit_clip_live`]), to close when it's done.
    live_edit: bool,
    /// Colours given to new tracks: the UI sets its theme's palette (grey
    /// until then).
    track_palette: Vec<Color>,
}

impl ProjectState {
    pub fn new(engine: Engine) -> Self {
        let sample_rate = engine.config().sample_rate;
        let edit_session = EditSession::new(Edit::new(DEFAULT_BPM), engine)
            .expect("an empty edit always compiles");
        Self {
            edit_session,
            sources: SourceRegistry::new(sample_rate),
            output: AudioOutput::default(),
            settings: Settings::default(),
            transport: Transport::new(),
            preview: Preview::default(),
            monitor: Monitor::default(),
            view: DocumentView::new(),
            session: Session::default(),
            history: History::default(),
            pending_actions: Vec::new(),
            batching: false,
            grid: GridService::new(),
            live_edit: false,
            track_palette: Vec::new(),
        }
    }

    /// Update each frame the state: follow the engine and the file loaders,
    /// run what was put off. Says what changed, for a UI drawing only that.
    pub fn update(&mut self) -> Updated {
        let was_playing = self.is_playing();
        let preview = self.preview.playback;
        let handled = !self.pending_actions.is_empty();
        self.handle_pending_actions();
        let audio = (self.audio_lost(), self.output.error.clone());
        self.watch_audio();
        let audio_changed = audio != (self.audio_lost(), self.output.error.clone());
        let loaded = self.sources.poll_loaded();
        let any_loaded = !loaded.is_empty();
        for (id, data) in loaded {
            let result = self.edit_session.set_source(id, data);
            report(result);
        }
        self.sync_time_signature();
        if self.transport.is_playing() {
            self.transport.playhead = self.edit_session.position();
        }
        let engine = self.edit_session.engine();
        self.preview
            .follow(engine.preview_position(), engine.is_previewing());
        self.monitor.update(&mut self.edit_session);
        Updated {
            project: handled || audio_changed || any_loaded,
            // Once more as it stops, for the UI to show it stopped.
            playback: was_playing || self.is_playing() || preview != self.preview.playback,
        }
    }

    /// Whether the transport or the preview plays.
    pub fn is_playing(&self) -> bool {
        self.transport.is_playing() || self.preview.playback == PlaybackState::Playing
    }

    // Engine plumbing
    fn edit(&self) -> &Edit {
        self.edit_session.edit()
    }

    fn perform(&mut self, cmd: impl EditCommand + 'static) {
        let steps = self.steps_recorded();
        let result = self.edit_session.perform(cmd);
        report(result);
        self.track_steps(steps);
    }

    /// Run `f` as one undo step (or as part of the open batch), its
    /// changes reaching the engine at once.
    fn transaction(&mut self, label: &'static str, f: impl FnOnce(&mut Self)) {
        self.edit_session.hold_effects();
        if self.batching {
            f(self);
        } else {
            let steps = self.steps_recorded();
            self.edit_session.begin_transaction(label);
            f(self);
            self.edit_session.commit_transaction();
            self.track_steps(steps);
        }
        report(self.edit_session.release_effects());
    }

    fn steps_recorded(&self) -> u64 {
        self.edit_session.undo_manager().steps_recorded()
    }

    /// If a step was recorded since `steps` were, keep `selection_undo` in
    /// step with it (see [`Self::selection_step`]).
    fn track_steps(&mut self, steps: u64) {
        if self.steps_recorded() == steps {
            return;
        }
        let depth = self.edit_session.undo_manager().undo_len();
        self.history.recorded(depth);
    }

    /// Run `f`, an edit that moves the clip selection: undoing it brings the
    /// selection from before it back (redoing, the one it left).
    pub(super) fn selection_step(&mut self, f: impl FnOnce(&mut Self)) {
        let (before, steps) = (self.session.clip_selection.clone(), self.steps_recorded());
        f(self);
        if self.steps_recorded() != steps && self.session.clip_selection != before {
            self.history.moved_selection(before);
        }
    }

    /// The channel of a track, a group (addressed by its bus's ID) or the
    /// master.
    fn channel_ref(&self, id: TrackId) -> ChannelRef {
        if id == MASTER_TRACK_ID {
            ChannelRef::Master
        } else if self.is_group(id) {
            ChannelRef::Bus(BusId(id.0))
        } else {
            ChannelRef::Track(id)
        }
    }

    pub fn new_clip_id(&mut self) -> ClipId {
        self.edit_session.create(|e| ClipId(e.next_id()))
    }

    // Selection and views
    /// Set track selected
    pub fn select_track(&mut self, id: &TrackId) {
        self.session.selected_tracks = vec![*id];
    }
    pub fn clear_track_selection(&mut self) {
        self.session.selected_tracks.clear();
    }
    pub fn selected_track(&self) -> Option<TrackRow> {
        let id = *self.session.selected_tracks.first()?;
        if id == MASTER_TRACK_ID {
            return Some(self.master_track());
        }
        if self.is_group(id) {
            return self.group_row(id);
        }
        self.track_index(id).and_then(|i| self.track_from_index(i))
    }
    /// Add the row to the selection, or take it out.
    pub fn toggle_track_selected(&mut self, id: TrackId) {
        if let Some(i) = self.session.selected_tracks.iter().position(|t| *t == id) {
            self.session.selected_tracks.remove(i);
        } else {
            self.session.selected_tracks.push(id);
        }
    }
    /// Select the rows from the last selected one to `id`, in display order.
    pub fn select_rows_to(&mut self, id: TrackId) {
        let rows: Vec<TrackId> = self.rows().iter().map(|r| r.id).collect();
        let anchor = self
            .session
            .selected_tracks
            .last()
            .and_then(|a| rows.iter().position(|r| r == a));
        let Some((from, to)) = anchor.zip(rows.iter().position(|r| *r == id)) else {
            return self.select_track(&id);
        };
        let range = if from <= to { from..=to } else { to..=from };
        self.session.selected_tracks = rows[range].to_vec();
    }
    /// Get all tracks
    pub fn tracks(&self) -> impl Iterator<Item = TrackRow> + use<> {
        let refs: Vec<_> = (0..self.track_count())
            .filter_map(|i| self.track_from_index(i))
            .collect();
        refs.into_iter()
    }
    pub fn master_track(&self) -> TrackRow {
        let view = &self.view.tracks[&MASTER_TRACK_ID];
        let master = &self.edit().master;
        TrackRow {
            id: MASTER_TRACK_ID,
            clips: Vec::new(),
            muted: master.muted,
            volume: master.volume.get(),
            pan: master.pan.get(),
            armed: false,
            name: view.name.clone(),
            height: view.height,
            collapsed: view.collapsed,
            color: view.color,
            selected: self.session.selected_tracks.contains(&MASTER_TRACK_ID),
            solo: TrackSoloState::NotSoloing,
            silenced: master.muted,
            first_track_index: 0,
            kind: TrackKind::Audio,
            depth: 0,
        }
    }
    pub fn selected_tracks(&self) -> &Vec<TrackId> {
        &self.session.selected_tracks
    }
    /// Positions (in beats) edits snap to besides the grid: the edges of the
    /// clips not in `exclude`, the loop edges and the edit cursor.
    pub fn snap_targets(&self, exclude: &[ClipId]) -> Vec<BeatPos> {
        let bpm = self.bpm();
        let (loop_start, loop_end) = self.transport.loop_range;
        self.tracks()
            .flat_map(|t| t.clips)
            .filter(|c| !exclude.contains(&c.id))
            .flat_map(|c| [c.position, c.end(bpm)])
            .chain([loop_start, loop_end, self.transport.edit_cursor])
            .collect()
    }
    /// Where the arrangement's content ends, in beats: the last clip end,
    /// the loop end while looping, or the playhead, whichever is furthest.
    pub fn arrangement_end(&self) -> BeatPos {
        let loop_end = if self.transport.looping {
            self.transport.loop_range.1
        } else {
            BeatPos::ZERO
        };
        // From the edit's clips, those shown: building the tracks' rows
        // would copy them all.
        self.edit()
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .filter(|clip| self.has_clip_view(clip))
            .map(|clip| clip.start + clip.length)
            .fold(self.transport.playhead.max(loop_end), BeatPos::max)
    }
    pub fn track_count(&self) -> usize {
        self.edit().tracks.len()
    }
    fn track_index(&self, id: TrackId) -> Option<usize> {
        self.edit().tracks.iter().position(|t| t.id == id)
    }
    /// Get mutable fields from track to be changed in place. Use `self.commit_track_view` to update the undo stack.
    pub fn track_view_mut(&mut self, id: &TrackId) -> &mut TrackView {
        self.view.tracks.entry(*id).or_default()
    }
    /// Commit changes made to the track mutable fields. Only the name is
    /// part of the undo history.
    pub fn commit_track_view(&mut self, id: &TrackId) {
        let Some(view) = self.view.tracks.get(id) else {
            return;
        };
        let name = view.name.clone();
        if let Ok(track) = self.edit().track(*id)
            && track.name != name
        {
            self.perform(RenameTrack::new(*id, name));
        } else if let Ok(group) = self.edit().bus(BusId(id.0))
            && group.name != name
        {
            self.perform(RenameBus::new(BusId(id.0), name));
        }
    }
    pub fn track_from_index(&self, index: usize) -> Option<TrackRow> {
        let track = self.edit().tracks.get(index)?;
        let any_solo = self.any_solo();
        let view = self
            .view
            .tracks
            .get(&track.id)
            .cloned()
            .unwrap_or_else(TrackView::new);
        Some(TrackRow {
            id: track.id,
            clips: track
                .clips
                .iter()
                .filter_map(|c| self.clip_view(c))
                .collect(),
            muted: track.channel.muted,
            volume: track.channel.volume.get(),
            pan: track.channel.pan.get(),
            armed: view.armed,
            name: track.name.clone(),
            height: view.height,
            collapsed: view.collapsed,
            color: view.color,
            selected: self.session.selected_tracks.contains(&track.id),
            solo: if !any_solo {
                TrackSoloState::NotSoloing
            } else if track.soloed {
                TrackSoloState::Solo
            } else if !track.channel.muted && !track.channel.audible() {
                // Silenced by another solo (a soloed group lets it through).
                TrackSoloState::Soloing
            } else {
                TrackSoloState::NotSoloing
            },
            silenced: !self.track_heard(track),
            first_track_index: index,
            kind: TrackKind::Audio,
            depth: self.edit().buses_along(track.output).len(),
        })
    }

    // History management
    /// Group the following changes into one undo step, until `commit_batch`,
    /// reaching the engine then (but for a live edit's, heard as made).
    pub fn begin_batch(&mut self) {
        if !self.live_edit {
            self.edit_session.hold_effects();
        }
        self.batching = true;
        self.history.batch_start = Some(self.steps_recorded());
        self.edit_session.begin_transaction("Edit");
    }
    /// Close the undo step opened by `begin_batch`.
    pub fn commit_batch(&mut self) {
        self.batching = false;
        self.edit_session.commit_transaction();
        if let Some(steps) = self.history.batch_start.take() {
            self.track_steps(steps);
        }
        if !self.live_edit {
            report(self.edit_session.release_effects());
        }
    }
    /// Undo last action, and bring back the clip selection from before it.
    /// Does nothing if there is no action.
    pub fn undo(&mut self) {
        let result = self.edit_session.undo();
        if matches!(result, Ok(true)) {
            self.history.undone(&mut self.session.clip_selection);
        }
        report(result);
        self.after_history_change();
    }
    /// Redo last action, and its clip selection. Does nothing if there is no
    /// action.
    pub fn redo(&mut self) {
        let result = self.edit_session.redo();
        if matches!(result, Ok(true)) {
            self.history.redone(&mut self.session.clip_selection);
        }
        report(result);
        self.after_history_change();
    }
    fn after_history_change(&mut self) {
        self.sync_time_signature();
        // Renames may have been undone: show the engine's names.
        for track in &self.edit_session.edit().tracks {
            if let Some(view) = self.view.tracks.get_mut(&track.id) {
                view.name = track.name.clone();
            }
        }
        self.session
            .selected_tracks
            .retain(|id| *id == MASTER_TRACK_ID || self.edit_session.edit().track(*id).is_ok());
        self.prune_clip_selection();
    }
    /// What the audio files clips use take, and the engine's copies.
    /// Whether a file clips use is still decoding: its waveform grows,
    /// its rhythm and samples are to come.
    pub fn sources_loading(&self) -> bool {
        self.sources.loading()
    }

    pub fn source_memory(&self) -> crate::dev::memory::SourceMemory {
        self.sources.memory()
    }

    /// Whether there is still actions to undo
    pub fn can_undo(&self) -> bool {
        self.edit_session.undo_manager().can_undo()
    }
    /// Whether there is still actions to redo
    pub fn can_redo(&self) -> bool {
        self.edit_session.undo_manager().can_redo()
    }
    /// Steps there are to undo, and the last one's name.
    #[cfg(any(test, feature = "test-support"))]
    pub fn undo_history(&self) -> (usize, Option<&'static str>) {
        let undo = self.edit_session.undo_manager();
        (undo.undo_len(), undo.undo_label())
    }

    /// To make sure some action do not conflict, pending actions are handled during state updates
    fn handle_pending_actions(&mut self) {
        for pending in take(&mut self.pending_actions) {
            match pending {
                ProjectStatePendingAction::DeleteTrack { id } => {
                    // A group goes with everything inside.
                    if self.is_group(id) {
                        self.delete_group(id);
                        continue;
                    }
                    let Some(pos) = self.track_index(id) else {
                        continue;
                    };
                    if self.session.selected_tracks.contains(&id) {
                        self.session.selected_tracks.clear();
                        if let Some(prev) = self.edit().tracks.get(pos.saturating_sub(1))
                            && prev.id != id
                        {
                            self.session.selected_tracks.push(prev.id);
                        }
                    }
                    self.remove_track(id);
                }
            }
        }
    }
}

/// Edit errors mean the UI asked for something stale (e.g. a clip that was
/// just deleted). Nothing to recover; make them visible in debug builds.
fn report<T>(result: Result<T, EditError>) {
    if let Err(e) = result {
        eprintln!("Edit failed: {e}");
    }
}
