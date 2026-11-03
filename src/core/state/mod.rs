mod audio;
mod clip_ops;
mod clipboard;
mod clips;
mod effects;
mod export;
mod groups;
mod mixer;
mod project;
mod selection;
mod sources;
#[cfg(test)]
mod tests;
mod tracks;
mod transport;
use crate::{
    audio::host::AudioHost,
    config::settings::Settings,
    core::{
        clip::AudioClip,
        graph_monitor::GraphMonitor,
        grid::GridService,
        metrics::{AudioMetrics, GlobalMetrics},
        state::{clipboard::Clipboard, selection::ClipSelection, sources::SourceRegistry},
        track::{TRACK_COLLAPSED_HEIGHT, TrackKind, TrackRow, TrackSoloState, TrackView},
    },
};
use egui::Color32;
use std::{
    collections::{HashMap, HashSet},
    mem::take,
    path::PathBuf,
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};

pub use export::ExportSource;
pub use groups::RowTarget;
pub use selection::SelectionBounds;
use tonique_engine::{
    edit::{
        BusId, ChannelRef, ClipId, Edit, EditError, EditSession, PluginId, TrackId,
        commands::{EditCommand, RenameBus, RenameTrack},
    },
    engine::Engine,
    time::BeatPos,
};

/// The master strip, addressed like a track by the UI. The engine never
/// hands out ID 0.
pub const MASTER_TRACK_ID: TrackId = TrackId(0);

const DEFAULT_BPM: f64 = 120.;
/// Length of the loop region in a new project.
const DEFAULT_LOOP_BARS: f64 = 4.;
/// A new track's name; `#` is its number.
pub const TRACK_NAME: &str = "# Audio Track";
/// How often to try opening an output again while there's none.
const AUDIO_RETRY: Duration = Duration::from_secs(2);
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaybackState {
    Paused,
    Playing,
}

/// The project as the UI sees it. The engine's [`EditSession`] owns the
/// document and its undo history; this adds UI-only state (selection, track
/// colors and heights, effect editors, meters) on top.
pub struct ProjectState {
    session: EditSession,
    sources: SourceRegistry,
    /// `None` without an audio device (e.g. in tests).
    audio: Option<AudioHost>,
    /// Applied preferences; change them with `apply_settings`.
    settings: Settings,
    /// Why the last audio restart failed, if it did.
    pub audio_error: Option<String>,
    /// When to try opening the output again, while there's none.
    reconnect_at: Option<Instant>,
    /// Whether an output can be opened, checked off the UI thread before
    /// reconnecting.
    probe: Option<Receiver<bool>>,

    /// Where the transport is, in beats (follows the engine while playing).
    playhead: BeatPos,
    /// Where the user last clicked in the arrangement, in beats. Playback
    /// starts and returns here, and edits (cuts) happen here.
    edit_cursor: BeatPos,
    playback_state: PlaybackState,
    /// Whether the timeline scrolls to keep the playhead in view.
    follow_playhead: bool,
    preview_playback_state: PlaybackState,
    preview_position: usize,
    /// The file last previewed.
    preview_path: Option<PathBuf>,
    /// Linear gain of the preview.
    preview_gain: f32,
    pub metrics: GlobalMetrics,
    /// Live graph data for the graph view (measured only while it's shown).
    pub graph: GraphMonitor,
    /// Whether the graph monitor measures: only while the graph is shown.
    monitor_graph: bool,

    /// UI-only track fields; kept for deleted tracks so undo restores them.
    views: HashMap<TrackId, TrackView>,
    /// Effects shown folded. Like a track's `collapsed`, a view setting:
    /// saved, not undoable.
    collapsed_effects: HashSet<PluginId>,
    selected_tracks: Vec<TrackId>,
    clip_selection: ClipSelection,
    clipboard: Clipboard,

    pending_actions: Vec<ProjectStatePendingAction>,
    batching: bool,
    /// For each step of the edit's undo history, in step with it: the clip
    /// selection before it, when the step moved the selection (a duplicate
    /// selects the copies). Undoing it brings that selection back.
    selection_undo: Vec<Option<ClipSelection>>,
    /// The same for the undone steps: the selection they left, for redoing.
    selection_redo: Vec<Option<ClipSelection>>,
    /// The steps recorded when the open batch began.
    batch_steps: Option<u64>,

    pub grid: GridService,
    metronome: bool,
    /// Loop region in beats; playback wraps over it while `looping`.
    loop_range: (BeatPos, BeatPos),
    looping: bool,
    /// An undo step was opened for a clip edit heard as it's made (see
    /// [`Self::edit_clip_live`]), to close when it's done.
    live_edit: bool,

    /// A clip being trimmed, as it will be once committed: shown instead
    /// of the edit's (see [`Self::shown_clip`]).
    resized_clip: Option<AudioClip>,
    /// Colours given to new tracks: the UI sets its theme's palette (grey
    /// until then).
    track_palette: Vec<Color32>,
}

impl ProjectState {
    pub fn new(engine: Engine) -> Self {
        let sample_rate = engine.config().sample_rate;
        let session = EditSession::new(Edit::new(DEFAULT_BPM), engine)
            .expect("an empty edit always compiles");
        let mut master = TrackView::new();
        master.name = "Master".into();
        Self {
            session,
            sources: SourceRegistry::new(sample_rate),
            playhead: BeatPos::ZERO,
            audio: None,
            settings: Settings::default(),
            audio_error: None,
            reconnect_at: None,
            probe: None,
            edit_cursor: BeatPos::ZERO,
            playback_state: PlaybackState::Paused,
            follow_playhead: false,
            preview_playback_state: PlaybackState::Paused,
            preview_position: 0,
            preview_path: None,
            preview_gain: 1.,
            metrics: GlobalMetrics::new(),
            graph: GraphMonitor::new(),
            monitor_graph: false,
            views: HashMap::from([(MASTER_TRACK_ID, master)]),
            collapsed_effects: HashSet::new(),
            selected_tracks: Vec::new(),
            clip_selection: ClipSelection::default(),
            clipboard: Clipboard::default(),
            pending_actions: Vec::new(),
            batching: false,
            selection_undo: Vec::new(),
            selection_redo: Vec::new(),
            batch_steps: None,
            resized_clip: None,
            grid: GridService::new(),
            loop_range: (
                BeatPos::ZERO,
                BeatPos(DEFAULT_LOOP_BARS * GridService::new().beats_per_bar()),
            ),
            looping: false,
            live_edit: false,
            track_palette: Vec::new(),
            metronome: false,
        }
    }

    /// Update each frame the state
    pub fn update(&mut self) {
        self.handle_pending_actions();
        self.watch_audio();
        for (id, data) in self.sources.poll_loaded() {
            let result = self.session.set_source(id, data);
            report(result);
        }
        self.sync_time_signature();
        if self.playback_state == PlaybackState::Playing {
            self.playhead = self.session.position();
        }
        let engine = self.session.engine();
        if let Some(pos) = engine.preview_position() {
            self.preview_position = pos;
        }
        if !engine.is_previewing() {
            // Played to the end
            self.preview_playback_state = PlaybackState::Paused;
        }
        self.update_metrics();
        self.graph
            .update(self.session.engine_mut(), self.monitor_graph);
    }
    /// Measure the engine's graph (for the graph view) from the next update.
    pub fn set_monitor_graph(&mut self, on: bool) {
        self.monitor_graph = on;
    }

    // Engine plumbing
    fn edit(&self) -> &Edit {
        self.session.edit()
    }

    fn perform(&mut self, cmd: impl EditCommand + 'static) {
        let steps = self.steps_recorded();
        let result = self.session.perform(cmd);
        report(result);
        self.track_steps(steps);
    }

    /// Run `f` as one undo step (or as part of the open batch), its
    /// changes reaching the engine at once.
    fn transaction(&mut self, label: &'static str, f: impl FnOnce(&mut Self)) {
        self.session.hold_effects();
        if self.batching {
            f(self);
        } else {
            let steps = self.steps_recorded();
            self.session.begin_transaction(label);
            f(self);
            self.session.commit_transaction();
            self.track_steps(steps);
        }
        report(self.session.release_effects());
    }

    fn steps_recorded(&self) -> u64 {
        self.session.undo_manager().steps_recorded()
    }

    /// If a step was recorded since `steps` were, keep `selection_undo` in
    /// step with it (see [`Self::selection_step`]).
    fn track_steps(&mut self, steps: u64) {
        if self.steps_recorded() == steps {
            return;
        }
        self.selection_undo.push(None);
        self.selection_redo.clear();
        // The history forgets its oldest steps past its depth.
        let excess = self
            .selection_undo
            .len()
            .saturating_sub(self.session.undo_manager().undo_len());
        self.selection_undo.drain(..excess);
    }

    /// Run `f`, an edit that moves the clip selection: undoing it brings the
    /// selection from before it back (redoing, the one it left).
    pub(super) fn selection_step(&mut self, f: impl FnOnce(&mut Self)) {
        let (before, steps) = (self.clip_selection.clone(), self.steps_recorded());
        f(self);
        if self.steps_recorded() != steps
            && self.clip_selection != before
            && let Some(last) = self.selection_undo.last_mut()
        {
            *last = Some(before);
        }
    }

    /// Forget the selections of the history, along with it.
    fn clear_selection_history(&mut self) {
        self.selection_undo.clear();
        self.selection_redo.clear();
        self.batch_steps = None;
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
        self.session.create(|e| ClipId(e.next_id()))
    }

    // Selection and views
    /// Set track selected
    pub fn select_track(&mut self, id: &TrackId) {
        self.selected_tracks = vec![*id];
    }
    pub fn clear_track_selection(&mut self) {
        self.selected_tracks.clear();
    }
    pub fn selected_track(&self) -> Option<TrackRow> {
        let id = *self.selected_tracks.first()?;
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
        if let Some(i) = self.selected_tracks.iter().position(|t| *t == id) {
            self.selected_tracks.remove(i);
        } else {
            self.selected_tracks.push(id);
        }
    }
    /// Select the rows from the last selected one to `id`, in display order.
    pub fn select_rows_to(&mut self, id: TrackId) {
        let rows: Vec<TrackId> = self.rows().iter().map(|r| r.id).collect();
        let anchor = self
            .selected_tracks
            .last()
            .and_then(|a| rows.iter().position(|r| r == a));
        let Some((from, to)) = anchor.zip(rows.iter().position(|r| *r == id)) else {
            return self.select_track(&id);
        };
        let range = if from <= to { from..=to } else { to..=from };
        self.selected_tracks = rows[range].to_vec();
    }
    /// Get all tracks
    pub fn tracks(&self) -> impl Iterator<Item = TrackRow> + use<> {
        let refs: Vec<_> = (0..self.track_count())
            .filter_map(|i| self.track_from_index(i))
            .collect();
        refs.into_iter()
    }
    pub fn master_track(&self) -> TrackRow {
        let view = &self.views[&MASTER_TRACK_ID];
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
            selected: self.selected_tracks.contains(&MASTER_TRACK_ID),
            solo: TrackSoloState::NotSoloing,
            silenced: master.muted,
            first_track_index: 0,
            kind: TrackKind::Audio,
            depth: 0,
        }
    }
    pub fn selected_tracks(&self) -> &Vec<TrackId> {
        &self.selected_tracks
    }
    /// Positions (in beats) edits snap to besides the grid: the edges of the
    /// clips not in `exclude`, the loop edges and the edit cursor.
    pub fn snap_targets(&self, exclude: &[ClipId]) -> Vec<BeatPos> {
        let bpm = self.bpm();
        let (loop_start, loop_end) = self.loop_range;
        self.tracks()
            .flat_map(|t| t.clips)
            .filter(|c| !exclude.contains(&c.id))
            .flat_map(|c| [c.position, c.end(bpm)])
            .chain([loop_start, loop_end, self.edit_cursor])
            .collect()
    }
    /// Where the arrangement's content ends, in beats: the last clip end,
    /// the loop end while looping, or the playhead, whichever is furthest.
    pub fn arrangement_end(&self) -> BeatPos {
        let bpm = self.bpm();
        let loop_end = if self.looping {
            self.loop_range.1
        } else {
            BeatPos::ZERO
        };
        self.tracks()
            .flat_map(|t| t.clips)
            .map(|c| c.end(bpm))
            .fold(self.playhead.max(loop_end), BeatPos::max)
    }
    pub fn track_count(&self) -> usize {
        self.edit().tracks.len()
    }
    fn track_index(&self, id: TrackId) -> Option<usize> {
        self.edit().tracks.iter().position(|t| t.id == id)
    }
    /// Get mutable fields from track to be changed in place. Use `self.commit_track_view` to update the undo stack.
    pub fn track_view_mut(&mut self, id: &TrackId) -> &mut TrackView {
        self.views.entry(*id).or_insert_with(TrackView::new)
    }
    /// Commit changes made to the track mutable fields. Only the name is
    /// part of the undo history.
    pub fn commit_track_view(&mut self, id: &TrackId) {
        let Some(view) = self.views.get(id) else {
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
            .views
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
            selected: self.selected_tracks.contains(&track.id),
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
            self.session.hold_effects();
        }
        self.batching = true;
        self.batch_steps = Some(self.steps_recorded());
        self.session.begin_transaction("Edit");
    }
    /// Close the undo step opened by `begin_batch`.
    pub fn commit_batch(&mut self) {
        self.drop_stray_crossfades();
        self.batching = false;
        self.session.commit_transaction();
        if let Some(steps) = self.batch_steps.take() {
            self.track_steps(steps);
        }
        if !self.live_edit {
            report(self.session.release_effects());
        }
    }
    /// Undo last action, and bring back the clip selection from before it.
    /// Does nothing if there is no action.
    pub fn undo(&mut self) {
        let result = self.session.undo();
        if matches!(result, Ok(true))
            && let Some(entry) = self.selection_undo.pop()
        {
            let after = entry.map(|before| std::mem::replace(&mut self.clip_selection, before));
            self.selection_redo.push(after);
        }
        report(result);
        self.after_history_change();
    }
    /// Redo last action, and its clip selection. Does nothing if there is no
    /// action.
    pub fn redo(&mut self) {
        let result = self.session.redo();
        if matches!(result, Ok(true))
            && let Some(entry) = self.selection_redo.pop()
        {
            let before = entry.map(|after| std::mem::replace(&mut self.clip_selection, after));
            self.selection_undo.push(before);
        }
        report(result);
        self.after_history_change();
    }
    fn after_history_change(&mut self) {
        self.sync_time_signature();
        // Renames may have been undone: show the engine's names.
        for track in &self.session.edit().tracks {
            if let Some(view) = self.views.get_mut(&track.id) {
                view.name = track.name.clone();
            }
        }
        self.selected_tracks
            .retain(|id| *id == MASTER_TRACK_ID || self.session.edit().track(*id).is_ok());
        self.prune_clip_selection();
    }
    /// Whether there is still actions to undo
    pub fn can_undo(&self) -> bool {
        self.session.undo_manager().can_undo()
    }
    /// Whether there is still actions to redo
    pub fn can_redo(&self) -> bool {
        self.session.undo_manager().can_redo()
    }
    /// Steps there are to undo, and the last one's name.
    #[cfg(test)]
    pub fn undo_history(&self) -> (usize, Option<&'static str>) {
        let undo = self.session.undo_manager();
        (undo.undo_len(), undo.undo_label())
    }

    /// Forget the clips (and held peaks) a track's meter shows.
    pub fn reset_meter_clip(&mut self, id: TrackId) {
        if id == MASTER_TRACK_ID {
            self.metrics.master.reset_clip();
        }
        if let Some(metrics) = self.metrics.tracks.get_mut(&id) {
            metrics.reset_clip();
        }
    }

    fn update_metrics(&mut self) {
        let edit = self.session.edit();
        self.metrics.master.update(edit.master.meter(), true);
        self.metrics
            .tracks
            .insert(MASTER_TRACK_ID, self.metrics.master.clone());
        let channels = edit
            .tracks
            .iter()
            .map(|t| (t.id, &t.channel))
            .chain(edit.buses.iter().map(|b| (TrackId(b.id.0), &b.channel)));
        for (id, channel) in channels {
            self.metrics
                .tracks
                .entry(id)
                .or_insert_with(AudioMetrics::new)
                .update(channel.meter(), false);
        }
        self.metrics.latency = self.session.engine().cpu_load();
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
                    if self.selected_tracks.contains(&id) {
                        self.selected_tracks.clear();
                        if let Some(prev) = self.edit().tracks.get(pos.saturating_sub(1))
                            && prev.id != id
                        {
                            self.selected_tracks.push(prev.id);
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
