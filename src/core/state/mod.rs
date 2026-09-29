mod clip_ops;
mod clipboard;
mod groups;
mod project;
mod selection;
mod sources;
#[cfg(test)]
mod tests;
use crate::{
    audio::{
        host::{AudioHost, start_audio},
        preview::FilePreview,
    },
    config::settings::Settings,
    core::{
        clip::ClipCore,
        graph_monitor::GraphMonitor,
        grid::GridService,
        metrics::{AudioMetrics, GlobalMetrics},
        state::{
            clip_ops::{ClipOp, TrackClips},
            clipboard::Clipboard,
            selection::ClipSelection,
            sources::SourceRegistry,
        },
        track::{
            MutableTrackCore, TRACK_CLOSED_HEIGHT, TrackKind, TrackReferenceCore, TrackSoloState,
        },
    },
    ui::{
        effect::UIEffect,
        effects::{EffectId, create_effect_from_id},
        theme::Theme,
    },
};
use egui::Color32;
use std::{collections::HashMap, mem::take, path::PathBuf};

pub use groups::RowTarget;
pub use selection::SelectionBounds;
use tonique_engine::{
    edit::{
        BusId, ChannelRef, Clip, ClipContent, ClipId, Edit, EditError, EditSession, Output, Plugin,
        PluginId, Track, TrackId,
        commands::{
            AddClip, AddPlugin, AddTrack, EditCommand, MoveClip, RemoveClip, RemovePlugin,
            RenameBus, RenameTrack, ResizeClip, SetBypass, SetMute, SetParam, SetSolo, SetTempoMap,
        },
    },
    engine::Engine,
    time::BeatPos,
};

/// The master strip, addressed like a track by the UI. The engine never
/// hands out ID 0.
pub const MASTER_TRACK_ID: TrackId = TrackId(0);

const DEFAULT_BPM: f64 = 120.;
/// Length of the loop region in a new project.
const DEFAULT_LOOP_BARS: f32 = 4.;
const TRACK_NAME: &str = "# Audio Track";
/// Smallest height of an open row (its controls must fit).
pub const MIN_OPEN_HEIGHT: f32 = TRACK_CLOSED_HEIGHT + 25.;

#[derive(Clone, Debug)]
enum ProjectStatePendingAction {
    DeleteTrack { id: TrackId },
}

/// What the central panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CentralView {
    Timeline,
    /// The engine's processing graph, live.
    Graph,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaybackState {
    Paused,
    Playing,
}

/// The project as the UI sees it. The engine's [`EditSession`] owns the
/// document and its undo history; this adds UI-only state (selection, track
/// colors and heights, effect editors, meters) on top.
pub struct ToniqueProjectState {
    session: EditSession,
    sources: SourceRegistry,
    /// `None` without an audio device (e.g. in tests).
    audio: Option<AudioHost>,
    /// Applied preferences; change them with `apply_settings`.
    settings: Settings,
    /// Why the last audio restart failed, if it did.
    pub audio_error: Option<String>,

    /// Where the transport is, in beats (follows the engine while playing).
    playhead: f32,
    /// Where the user last clicked in the arrangement, in beats. Playback
    /// starts and returns here, and edits (cuts) happen here.
    edit_cursor: f32,
    playback_state: PlaybackState,
    /// Whether the timeline scrolls to keep the playhead in view.
    follow_playhead: bool,
    preview_playback_state: PlaybackState,
    preview_position: usize,
    pub metrics: GlobalMetrics,
    /// Live graph data for the graph view (measured only while it's shown).
    pub graph: GraphMonitor,
    pub central_view: CentralView,

    /// UI-only track fields; kept for deleted tracks so undo restores them.
    views: HashMap<TrackId, MutableTrackCore>,
    selected_tracks: Vec<TrackId>,
    clip_selection: ClipSelection,
    clipboard: Clipboard,
    /// Effect editors per track, in plugin order.
    effects: HashMap<TrackId, Vec<UIEffect>>,
    /// Editors of plugins not currently in a chain (removed, or not synced yet).
    detached_effects: HashMap<PluginId, UIEffect>,

    pending_actions: Vec<ProjectStatePendingAction>,
    batching: bool,

    pub grid: GridService,
    metronome: bool,
    /// Loop region in beats; playback wraps over it while `looping`.
    loop_range: (f32, f32),
    looping: bool,
    /// Tempo the engine's loop (in samples) was computed with.
    loop_bpm: f32,

    pub resized_clip: Option<(ClipId, f32, f32, f32)>,
    /// Colours given to new tracks: the installed theme's palette.
    track_palette: Vec<Color32>,
    // Panels
    pub left_panel_open: bool,
    pub bottom_panel_open: bool,
}

impl ToniqueProjectState {
    pub fn new(engine: Engine) -> Self {
        let sample_rate = engine.config().sample_rate;
        let session = EditSession::new(Edit::new(DEFAULT_BPM), engine)
            .expect("an empty edit always compiles");
        let mut master = MutableTrackCore::new();
        master.name = "Master".into();
        Self {
            session,
            sources: SourceRegistry::new(sample_rate),
            playhead: 0.,
            audio: None,
            settings: Settings::default(),
            audio_error: None,
            edit_cursor: 0.,
            playback_state: PlaybackState::Paused,
            follow_playhead: false,
            preview_playback_state: PlaybackState::Paused,
            preview_position: 0,
            metrics: GlobalMetrics::new(),
            graph: GraphMonitor::new(),
            central_view: CentralView::Timeline,
            views: HashMap::from([(MASTER_TRACK_ID, master)]),
            selected_tracks: Vec::new(),
            clip_selection: ClipSelection::default(),
            clipboard: Clipboard::default(),
            effects: HashMap::new(),
            detached_effects: HashMap::new(),
            pending_actions: Vec::new(),
            batching: false,
            resized_clip: None,
            grid: GridService::new(),
            loop_range: (
                0.,
                DEFAULT_LOOP_BARS * GridService::new().beats_per_bar() as f32,
            ),
            looping: false,
            loop_bpm: DEFAULT_BPM as f32,
            track_palette: Theme::dark().palette,
            left_panel_open: true,
            bottom_panel_open: false,
            metronome: false,
        }
    }

    /// Update each frame the state
    pub fn update(&mut self) {
        self.handle_pending_actions();
        for (id, data) in self.sources.poll_loaded() {
            let result = self.session.set_source(id, data);
            report(result);
        }
        // Tempo changes (edits, undo, redo) move the loop in samples.
        if self.looping && self.bpm() != self.loop_bpm {
            self.sync_loop();
        }
        if self.playback_state == PlaybackState::Playing {
            self.playhead = self.session.position().0 as f32;
        }
        let engine = self.session.engine();
        if let Some(pos) = engine.preview_position() {
            self.preview_position = pos;
        }
        if !engine.is_previewing() {
            // Played to the end
            self.preview_playback_state = PlaybackState::Paused;
        }
        self.sync_effects();
        self.update_metrics();
        let show_graph = self.central_view == CentralView::Graph;
        self.graph.update(self.session.engine_mut(), show_graph);
    }

    // Settings and audio output
    /// Take over the running audio output and the settings it was opened with.
    pub fn attach_audio(&mut self, audio: AudioHost, settings: Settings) {
        self.audio = Some(audio);
        self.settings = settings;
        self.apply_metronome_level();
    }
    pub fn audio(&self) -> Option<&AudioHost> {
        self.audio.as_ref()
    }
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    /// The engine running now (its settings may differ from the saved ones
    /// until an audio restart).
    pub fn engine_config(&self) -> tonique_engine::engine::EngineConfig {
        *self.session.engine().config()
    }
    pub fn cpu_load(&self) -> f32 {
        self.session.engine().cpu_load()
    }
    /// Save and apply `settings`, restarting audio if device or engine
    /// settings changed. If the new audio settings don't work, the previous
    /// ones are restored and `audio_error` says why.
    pub fn apply_settings(&mut self, settings: Settings) {
        let previous = std::mem::replace(&mut self.settings, settings);
        self.apply_metronome_level();
        if self.audio.is_some() && previous.audio_differs(&self.settings) {
            self.audio_error = None;
            if let Err(e) = self.restart_audio() {
                self.audio_error = Some(e);
                let attempted = std::mem::replace(&mut self.settings, previous);
                // Keep the non-audio changes.
                self.settings.ui_scale = attempted.ui_scale;
                self.settings.metronome_level = attempted.metronome_level;
                if let Err(e) = self.restart_audio() {
                    self.audio_error = Some(format!(
                        "{}; restoring the previous device failed too: {e}",
                        self.audio_error.take().unwrap_or_default()
                    ));
                }
            }
        }
        self.settings.save();
    }

    /// Reopen the output device and move the project to a fresh engine with
    /// the current settings. Keeps the undo history and the playhead; keeps
    /// playing unless the sample rate changed (files are converted again).
    fn restart_audio(&mut self) -> Result<(), String> {
        let position = self.playhead;
        let playing = self.playback_state == PlaybackState::Playing;
        self.pause_preview();
        // Release the device first: some drivers only allow one stream.
        self.audio = None;
        let (audio, engine) = start_audio(&self.settings).map_err(|e| e.to_string())?;
        let rate = engine.config().sample_rate;
        let rate_changed = rate != self.session.engine().config().sample_rate;
        let old_engine = self
            .session
            .replace_engine(engine)
            .map_err(|e| e.to_string())?;
        drop(old_engine);
        self.audio = Some(audio);
        self.sources.set_engine_rate(rate);
        self.graph.reset();
        self.seek(position);
        if playing && !rate_changed {
            self.play();
        } else {
            self.playback_state = PlaybackState::Paused;
        }
        Ok(())
    }

    // Engine plumbing
    fn edit(&self) -> &Edit {
        self.session.edit()
    }

    fn perform(&mut self, cmd: impl EditCommand + 'static) {
        let result = self.session.perform(cmd);
        report(result);
    }

    /// Run `f` as one undo step (or as part of the open batch).
    fn transaction(&mut self, label: &'static str, f: impl FnOnce(&mut Self)) {
        if self.batching {
            return f(self);
        }
        self.session.begin_transaction(label);
        f(self);
        self.session.commit_transaction();
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

    // Bpm
    pub fn set_bpm(&mut self, value: f32) {
        let old = self.bpm();
        if value <= 0. || value == old {
            return;
        }
        // Audio clips keep their length in seconds, so their length in beats
        // scales with the tempo.
        let resized: Vec<_> = self
            .edit()
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(move |c| (t.id, c)))
            .filter_map(|(t, c)| match c.content {
                ClipContent::Audio {
                    source_offset_s, ..
                } => {
                    let length = c.length * value as f64 / old as f64;
                    Some(ResizeClip::new(t, c.id, c.start, length, source_offset_s))
                }
                ClipContent::Midi { .. } => None,
            })
            .collect();
        let mut tempo = self.edit().tempo.clone();
        tempo.set_tempo(BeatPos(0.), value as f64);
        self.transaction("Change tempo", |s| {
            s.perform(SetTempoMap::new(tempo));
            for cmd in resized {
                s.perform(cmd);
            }
        });
    }

    pub fn bpm(&self) -> f32 {
        self.edit().tempo.bpm_at(BeatPos(0.)) as f32
    }
    // Playhead and edit cursor
    /// Move the playhead and the edit cursor there (ruler, playhead handle).
    pub fn seek(&mut self, beats: f32) {
        self.edit_cursor = beats.max(0.);
        self.seek_playhead(self.edit_cursor);
    }
    /// Place the edit cursor (a click in the arrangement). Doesn't interrupt
    /// playback; when stopped, the playhead follows so play starts there.
    pub fn set_edit_cursor(&mut self, beats: f32) {
        self.edit_cursor = beats.max(0.);
        if self.playback_state != PlaybackState::Playing {
            self.seek_playhead(self.edit_cursor);
        }
    }
    fn seek_playhead(&mut self, beats: f32) {
        self.playhead = beats;
        let result = self.session.seek(BeatPos(beats as f64));
        report(result);
    }
    pub fn playhead(&self) -> f32 {
        self.playhead
    }
    pub fn edit_cursor(&self) -> f32 {
        self.edit_cursor
    }
    pub fn follow_playhead(&self) -> bool {
        self.follow_playhead
    }
    pub fn set_follow_playhead(&mut self, follow: bool) {
        self.follow_playhead = follow;
    }
    // Transport state
    /// Stop playback and return the playhead to the edit cursor.
    pub fn stop(&mut self) {
        self.playback_state = PlaybackState::Paused;
        let result = self.session.stop();
        report(result);
        self.seek_playhead(self.edit_cursor);
    }
    pub fn play(&mut self) {
        self.playback_state = PlaybackState::Playing;
        self.pause_preview();
        let result = self.session.play();
        report(result);
    }
    pub fn pause_preview(&mut self) {
        self.preview_playback_state = PlaybackState::Paused;
        let result = self.session.engine_mut().preview_stop();
        report(result.map_err(EditError::from));
    }
    /// Play a file from the start. Ignored while the transport plays.
    pub fn play_preview(&mut self, path: PathBuf) {
        if self.playback_state == PlaybackState::Playing {
            return;
        }
        let Some(source) = FilePreview::open(path) else {
            return;
        };
        self.preview_position = 0;
        self.preview_playback_state = PlaybackState::Playing;
        let result = self.session.engine_mut().preview_play(Box::new(source), 0);
        report(result.map_err(EditError::from));
    }
    /// Continue the last previewed file from `pos` (in frames of the file).
    pub fn seek_preview(&mut self, pos: usize) {
        self.preview_position = pos;
        self.preview_playback_state = PlaybackState::Playing;
        let result = self.session.engine_mut().preview_seek(pos);
        report(result.map_err(EditError::from));
    }
    // Loop
    pub fn looping(&self) -> bool {
        self.looping
    }
    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
        self.sync_loop();
    }
    pub fn loop_range(&self) -> (f32, f32) {
        self.loop_range
    }
    /// Set the loop region (in any order). Empty regions are ignored.
    pub fn set_loop_range(&mut self, a: f32, b: f32) {
        let (start, end) = (a.min(b).max(0.), a.max(b).max(0.));
        if end > start && (start, end) != self.loop_range {
            self.loop_range = (start, end);
            self.sync_loop();
        }
    }
    /// Hand the loop to the engine. It counts in samples, so this must run
    /// again whenever the tempo changes.
    fn sync_loop(&mut self) {
        self.loop_bpm = self.bpm();
        let (start, end) = self.loop_range;
        let range = self
            .looping
            .then_some((BeatPos(start as f64), BeatPos(end as f64)));
        report(self.session.set_loop(range));
    }
    pub fn toggle_metronome(&mut self) {
        self.metronome = !self.metronome;
        self.apply_metronome_level();
    }
    fn apply_metronome_level(&self) {
        let gain = if self.metronome {
            self.settings.metronome_level
        } else {
            0.
        };
        self.edit().metronome.set(gain, 0.);
    }
    pub fn metronome(&self) -> bool {
        self.metronome
    }

    pub fn playback_state(&self) -> PlaybackState {
        self.playback_state
    }
    pub fn preview_playback_state(&self) -> PlaybackState {
        self.preview_playback_state
    }
    pub fn preview_position(&self) -> usize {
        self.preview_position
    }
    // Tracks
    /// Add track at the last position. Shortcut for `add_track_at`
    pub fn add_track(&mut self) -> TrackId {
        self.add_track_at(self.track_len())
    }
    /// Add track at specific index
    pub fn add_track_at(&mut self, index: usize) -> TrackId {
        let mut track = self.session.create(|e| Track::new(e, TRACK_NAME));
        // Inside the group its neighbours share, so no group is split.
        if let Some(group) = self.insertion_parent(index) {
            track.output = Output::Bus(BusId(group.0));
        }
        let id = track.id;
        let mut view = MutableTrackCore::new();
        view.color = self.next_track_color();
        self.views.insert(id, view);
        self.perform(AddTrack::at(track, index));
        id
    }

    /// The palette colour the fewest tracks use, the first one on ties, so
    /// new tracks go through the palette before repeating a colour.
    fn next_track_color(&self) -> Color32 {
        let uses = |color: Color32| {
            self.views
                .iter()
                .filter(|(id, view)| **id != MASTER_TRACK_ID && view.color == color)
                .count()
        };
        self.track_palette
            .iter()
            .copied()
            .min_by_key(|color| uses(*color))
            .unwrap_or(Color32::GRAY)
    }

    /// Colours for new tracks; the UI passes the installed theme's.
    pub fn set_track_palette(&mut self, palette: &[Color32]) {
        if self.track_palette != palette {
            self.track_palette = palette.to_vec();
        }
    }
    /// Duplicate track. New track is inserted after the current track.
    pub fn duplicate_track(&mut self, id: &TrackId) {
        let Ok(original) = self.edit().track(*id).cloned() else {
            return;
        };
        let index = self.track_index(*id).unwrap_or(self.track_len());
        let copy = self.session.create(|e| original.duplicate(e));
        let copy_id = copy.id;
        if let Some(view) = self.views.get(id).cloned() {
            self.views.insert(copy_id, view);
        }
        // Effect editors follow the plugins, which keep their order.
        for (from, to) in original.channel.plugins.iter().zip(&copy.channel.plugins) {
            if let Some(editor) = self.effect(*id, from.id) {
                let mut editor = editor.clone();
                editor.bind(copy_id, to);
                self.detached_effects.insert(to.id, editor);
            }
        }
        self.perform(AddTrack::at(copy, index + 1));
        self.sync_effects();
        self.select_track(&copy_id);
    }
    /// Delete a track
    pub fn delete_track(&mut self, id: &TrackId) {
        self.pending_actions
            .push(ProjectStatePendingAction::DeleteTrack { id: *id });
    }
    /// Close or open all tracks
    pub fn set_all_close(&mut self, close: bool) {
        let ids: Vec<TrackId> = self.views.keys().copied().collect();
        for id in ids {
            self.set_closed(&id, close);
        }
    }
    /// Close (collapse; fold, for a group) or open a row, going back to its
    /// previous height when opened.
    pub fn set_closed(&mut self, id: &TrackId, closed: bool) {
        let view = self.track_mut(id);
        if view.closed == closed {
            return;
        }
        if closed {
            view.open_height = view.height;
            view.height = TRACK_CLOSED_HEIGHT;
        } else {
            view.height = view.open_height.max(MIN_OPEN_HEIGHT);
        }
        view.closed = closed;
    }
    // Clips
    fn clip_ops(&mut self, label: &'static str, f: impl FnOnce(&mut Self, &mut Vec<ClipOp>)) {
        let mut ops = Vec::new();
        f(self, &mut ops);
        self.transaction(label, |s| {
            for op in ops {
                s.apply_clip_op(op);
            }
        });
    }

    fn track_clips<'a>(&self, track: TrackId, ops: &'a mut Vec<ClipOp>) -> TrackClips<'a> {
        TrackClips {
            track,
            clips: self.clip_views(track),
            bpm: self.bpm(),
            ops,
        }
    }

    /// Add clips and fix all overlaps on the track.
    pub fn add_clips(&mut self, track_id: &TrackId, mut clips: Vec<ClipCore>) {
        // Clips never start before the first beat.
        for clip in &mut clips {
            clip.position = clip.position.max(0.);
        }
        self.clip_ops("Add clips", |s, ops| {
            let mut clips_on_track = s.track_clips(*track_id, ops);
            clips_on_track.add(clips, &mut || s.new_clip_id());
        });
    }
    /// Move clip to a new position and a new track fixing all overlaps on this track.
    /// Clips never start before the first beat.
    pub fn move_clip(&mut self, id: &ClipId, to_track: &TrackId, to_pos: f32, ignore: &[ClipId]) {
        let Some((from, clip)) = self.find_clip(*id) else {
            return;
        };
        let to_pos = to_pos.max(0.);
        self.clip_ops("Move clips", |s, ops| {
            let mut moved = clip.clone();
            moved.position = to_pos;
            let mut ignore = ignore.to_vec();
            ignore.push(*id);
            let end = moved.end(s.bpm());
            let mut clips_on_track = s.track_clips(*to_track, ops);
            clips_on_track.carve(to_pos, end, &ignore, &mut || s.new_clip_id());
            ops.push(ClipOp::Move {
                from,
                to: *to_track,
                clip: *id,
                position: to_pos,
            });
        });
    }
    /// Delete clips for their ids
    pub fn delete_clips(&mut self, ids: &[ClipId]) {
        self.clip_ops("Delete clips", |s, ops| {
            for id in ids {
                if let Some((track, _)) = s.find_clip(*id) {
                    ops.push(ClipOp::Remove(track, *id));
                }
            }
        });
    }
    /// Cut clip located at position on given track. Does nothing it there is no clip.
    pub fn cut_clip_at(&mut self, track_id: &TrackId, position: f32) {
        self.clip_ops("Cut clip", |s, ops| {
            let mut clips_on_track = s.track_clips(*track_id, ops);
            clips_on_track.cut_at(position, &mut || s.new_clip_id());
        });
    }
    /// Duplicate clips fixing all overlaps on the tracks.
    pub fn duplicate_clips(&mut self, ids: &[ClipId], bounds: Option<(f32, f32)>) -> Vec<ClipId> {
        let mut copy_ids = Vec::new();
        self.clip_ops("Duplicate clips", |s, ops| {
            let tracks: Vec<_> = s.edit().tracks.iter().map(|t| t.id).collect();
            for track in tracks {
                let mut clips_on_track = s.track_clips(track, ops);
                let mut new_id = || s.new_clip_id();
                let copies = clips_on_track.duplicates(ids, bounds, &mut new_id);
                copy_ids.extend(copies.iter().map(|clip| clip.id));
                clips_on_track.add(copies, &mut new_id);
            }
        });
        copy_ids
    }
    /// Resize clip without computing overlap checks.
    /// Use `commit_resize_clip` to apply overlap checks and add to undo stack.
    pub fn resize_clip(&mut self, id: &ClipId, start: f32, end: f32, pos: f32) {
        self.resized_clip = Some((*id, start, end, pos));
    }
    /// Resize clip and perform overlap checks
    pub fn commit_resize_clip(&mut self, id: &ClipId, start: f32, end: f32, pos: f32) {
        self.resized_clip = None;
        let Some((track, clip)) = self.find_clip(*id) else {
            return;
        };
        self.clip_ops("Resize clip", |s, ops| {
            let mut resized = clip;
            resized.trim_start = start;
            resized.trim_end = end;
            resized.position = pos;
            let end = resized.end(s.bpm());
            let mut clips_on_track = s.track_clips(track, ops);
            clips_on_track.carve(pos, end, &[*id], &mut || s.new_clip_id());
            ops.push(ClipOp::Resize(track, resized));
        });
    }

    fn apply_clip_op(&mut self, op: ClipOp) {
        let bpm = self.bpm();
        match op {
            ClipOp::Add(track, clip) => {
                let clip = self.engine_clip(&clip, bpm);
                self.perform(AddClip::new(track, clip));
            }
            ClipOp::Remove(track, id) => self.perform(RemoveClip::new(track, id)),
            ClipOp::Resize(track, clip) => {
                let c = self.engine_clip(&clip, bpm);
                let ClipContent::Audio {
                    source_offset_s, ..
                } = c.content
                else {
                    return;
                };
                self.perform(ResizeClip::new(
                    track,
                    c.id,
                    c.start,
                    c.length,
                    source_offset_s,
                ));
            }
            ClipOp::Move {
                from,
                to,
                clip,
                position,
            } => self.perform(MoveClip::to_track(from, clip, to, BeatPos(position as f64))),
        }
    }

    /// The engine clip for a UI clip, registering its audio file if needed.
    fn engine_clip(&mut self, clip: &ClipCore, bpm: f32) -> Clip {
        let session = &mut self.session;
        let source = self
            .sources
            .get_or_insert(&clip.audio, || session.create(|e| e.new_source()));
        Clip {
            id: clip.id,
            start: BeatPos(clip.position as f64),
            length: (clip.end(bpm) - clip.position) as f64,
            fade_in_s: 0.005,
            fade_out_s: 0.005,
            content: ClipContent::Audio {
                source,
                source_offset_s: (clip.trim_start * clip.source_seconds()) as f64,
                gain: 1.,
            },
        }
    }

    /// The UI view of an engine clip. `None` for MIDI clips, which the UI
    /// can't show yet.
    fn clip_view(&self, clip: &Clip, bpm: f32) -> Option<ClipCore> {
        let ClipContent::Audio {
            source,
            source_offset_s,
            ..
        } = clip.content
        else {
            return None;
        };
        let audio = self.sources.info(source)?.clone();
        let seconds = audio.duration?.as_secs_f64();
        let trim_start = (source_offset_s / seconds) as f32;
        let trim_end = trim_start + (clip.length * 60. / bpm as f64 / seconds) as f32;
        Some(ClipCore {
            id: clip.id,
            audio,
            position: clip.start.0 as f32,
            trim_start,
            trim_end,
        })
    }

    fn clip_views(&self, track: TrackId) -> Vec<ClipCore> {
        let bpm = self.bpm();
        self.edit()
            .track(track)
            .map(|t| {
                t.clips
                    .iter()
                    .filter_map(|c| self.clip_view(c, bpm))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn find_clip(&self, id: ClipId) -> Option<(TrackId, ClipCore)> {
        let track = self.edit().tracks.iter().find(|t| t.clip(id).is_some())?;
        let clip = self.clip_view(track.clip(id)?, self.bpm())?;
        Some((track.id, clip))
    }

    // Effects
    /// Add a effect to the track
    pub fn add_effect(&mut self, id: &TrackId, effect_id: EffectId, index: usize) {
        let content = create_effect_from_id(effect_id);
        let plugin = self
            .session
            .create(|e| Plugin::new(e, content.plugin_kind()));
        let editor = UIEffect::new(content, *id, &plugin);
        self.detached_effects.insert(plugin.id, editor);
        self.perform(AddPlugin::at(self.channel_ref(*id), plugin, index));
        self.sync_effects();
    }
    pub fn remove_effects(&mut self, id: &TrackId, indexes: &[usize]) {
        let plugins: Vec<_> = self
            .effects
            .get(id)
            .map(|e| {
                indexes
                    .iter()
                    .filter_map(|i| e.get(*i).map(|e| e.plugin_id()))
                    .collect()
            })
            .unwrap_or_default();
        self.transaction("Remove effects", |s| {
            for plugin in plugins {
                s.perform(RemovePlugin::new(s.channel_ref(*id), plugin));
            }
        });
        self.sync_effects();
    }
    pub fn effects_mut(&mut self, id: &TrackId) -> Option<&mut [UIEffect]> {
        self.effects.get_mut(id).map(|e| e.as_mut_slice())
    }
    fn effect(&self, track: TrackId, plugin: PluginId) -> Option<&UIEffect> {
        self.effects
            .get(&track)
            .and_then(|e| e.iter().find(|e| e.plugin_id() == plugin))
            .or_else(|| self.detached_effects.get(&plugin))
    }

    /// Push power-button clicks to the engine, then match effect editors to
    /// the engine's plugin chains (which undo/redo may have changed).
    fn sync_effects(&mut self) {
        let mut toggled = Vec::new();
        for (track, editors) in self.effects.drain() {
            for mut editor in editors {
                if editor.take_toggled() {
                    toggled.push((track, editor.plugin_id(), !editor.enabled));
                }
                self.detached_effects.insert(editor.plugin_id(), editor);
            }
        }
        for (track, plugin, bypassed) in toggled {
            let cmd = SetBypass::new(self.channel_ref(track), plugin, bypassed);
            self.perform(cmd);
        }
        let chains: Vec<(TrackId, Vec<(PluginId, bool)>)> = self
            .edit()
            .tracks
            .iter()
            .map(|t| (t.id, &t.channel))
            .chain(
                self.edit()
                    .buses
                    .iter()
                    .map(|b| (TrackId(b.id.0), &b.channel)),
            )
            .chain([(MASTER_TRACK_ID, &self.edit().master)])
            .map(|(id, c)| (id, c.plugins.iter().map(|p| (p.id, p.bypassed)).collect()))
            .collect();
        for (track, plugins) in chains {
            let editors = plugins
                .into_iter()
                .filter_map(|(id, bypassed)| {
                    let mut editor = self.detached_effects.remove(&id)?;
                    editor.enabled = !bypassed;
                    Some(editor)
                })
                .collect();
            self.effects.insert(track, editors);
        }
    }

    // Mixer
    /// Set individual track volume. Changes are not saved in undo stack.
    pub fn set_volume(&mut self, id: TrackId, volume: f32) {
        if let Ok(c) = self.edit().channel(self.channel_ref(id)) {
            c.volume.set(volume);
        }
    }
    /// Set track volume and save in undo stack given `old_volume`.
    pub fn commit_volume(&mut self, id: TrackId, old_volume: f32, new_volume: f32) {
        let Ok(c) = self.edit().channel(self.channel_ref(id)) else {
            return;
        };
        // `SetParam` records the current value for undo: restore it first.
        c.volume.set(old_volume);
        let param = c.volume.id;
        self.perform(SetParam::new(param, new_volume));
    }
    /// Mute or unmute this track
    pub fn set_mute(&mut self, id: TrackId, mute: bool) {
        self.perform(SetMute::new(self.channel_ref(id), mute));
    }
    /// Toggle the solo button.
    /// Toggle the solo button of a track or a group. Without the modifier,
    /// every other solo is cleared.
    pub fn toggle_solo(&mut self, id: TrackId, modifier_pressed: bool) {
        let edit = self.edit();
        let soloed: Vec<(TrackId, bool)> = edit
            .tracks
            .iter()
            .map(|t| (t.id, t.soloed))
            .chain(edit.buses.iter().map(|b| (TrackId(b.id.0), b.soloed)))
            .collect();
        let Some(current) = soloed.iter().find(|(t, _)| *t == id).map(|(_, s)| *s) else {
            return;
        };
        let changes: Vec<(TrackId, bool)> = soloed
            .into_iter()
            .filter_map(|(t, was)| {
                let solo = if t == id {
                    !current
                } else {
                    modifier_pressed && was
                };
                (solo != was).then_some((t, solo))
            })
            .collect();
        self.transaction("Solo", |s| {
            for (target, solo) in changes {
                if s.is_group(target) {
                    s.perform(SetSolo::bus(BusId(target.0), solo));
                } else {
                    s.perform(SetSolo::new(target, solo));
                }
            }
        });
    }
    /// Whether any track or group is soloed.
    fn any_solo(&self) -> bool {
        let edit = self.edit();
        edit.tracks.iter().any(|t| t.soloed) || edit.buses.iter().any(|b| b.soloed)
    }
    // Selection and views
    /// Set track selected
    pub fn select_track(&mut self, id: &TrackId) {
        self.selected_tracks = vec![*id];
    }
    pub fn deselect(&mut self) {
        self.selected_tracks.clear();
    }
    pub fn selected_track(&self) -> Option<TrackReferenceCore> {
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
    pub fn tracks(&self) -> impl Iterator<Item = TrackReferenceCore> + use<> {
        let refs: Vec<_> = (0..self.track_len())
            .filter_map(|i| self.track_from_index(i))
            .collect();
        refs.into_iter()
    }
    pub fn master_track(&self) -> TrackReferenceCore {
        let view = &self.views[&MASTER_TRACK_ID];
        let master = &self.edit().master;
        TrackReferenceCore {
            id: MASTER_TRACK_ID,
            clips: Vec::new(),
            muted: master.muted,
            volume: master.volume.get(),
            arm: false,
            name: view.name.clone(),
            height: view.height,
            closed: view.closed,
            color: view.color,
            selected: self.selected_tracks.contains(&MASTER_TRACK_ID),
            solo: TrackSoloState::NotSoloing,
            index: 0,
            kind: TrackKind::Audio,
            depth: 0,
        }
    }
    pub fn selected_tracks(&self) -> &Vec<TrackId> {
        &self.selected_tracks
    }
    /// Positions (in beats) edits snap to besides the grid: the edges of the
    /// clips not in `exclude`, the loop edges and the edit cursor.
    pub fn snap_targets(&self, exclude: &[ClipId]) -> Vec<f32> {
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
    pub fn arrangement_end(&self) -> f32 {
        let bpm = self.bpm();
        let loop_end = if self.looping { self.loop_range.1 } else { 0. };
        self.tracks()
            .flat_map(|t| t.clips)
            .map(|c| c.end(bpm))
            .fold(self.playhead.max(loop_end), f32::max)
    }
    pub fn track_len(&self) -> usize {
        self.edit().tracks.len()
    }
    fn track_index(&self, id: TrackId) -> Option<usize> {
        self.edit().tracks.iter().position(|t| t.id == id)
    }
    /// Get mutable fields from track to be changed in place. Use `self.commit_track_mut` to update the undo stack.
    pub fn track_mut(&mut self, id: &TrackId) -> &mut MutableTrackCore {
        self.views.entry(*id).or_insert_with(MutableTrackCore::new)
    }
    /// Commit changes made to the track mutable fields. Only the name is
    /// part of the undo history.
    pub fn commit_track_mut(&mut self, id: &TrackId) {
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
    pub fn track_from_index(&self, index: usize) -> Option<TrackReferenceCore> {
        let track = self.edit().tracks.get(index)?;
        let any_solo = self.any_solo();
        let view = self
            .views
            .get(&track.id)
            .cloned()
            .unwrap_or_else(MutableTrackCore::new);
        let bpm = self.bpm();
        Some(TrackReferenceCore {
            id: track.id,
            clips: track
                .clips
                .iter()
                .filter_map(|c| self.clip_view(c, bpm))
                .collect(),
            muted: track.channel.muted,
            volume: track.channel.volume.get(),
            arm: view.arm,
            name: track.name.clone(),
            height: view.height,
            closed: view.closed,
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
            index,
            kind: TrackKind::Audio,
            depth: self.edit().buses_along(track.output).len(),
        })
    }

    // History management
    /// Group the following changes into one undo step, until `commit_batch`.
    pub fn begin_batch(&mut self) {
        self.batching = true;
        self.session.begin_transaction("Edit");
    }
    /// Close the undo step opened by `begin_batch`.
    pub fn commit_batch(&mut self) {
        self.batching = false;
        self.session.commit_transaction();
    }
    /// Undo last action. Does nothing if there is no action.
    pub fn undo(&mut self) {
        let result = self.session.undo();
        report(result);
        self.after_history_change();
    }
    /// Redo last action. Does nothing if there is no action.
    pub fn redo(&mut self) {
        let result = self.session.redo();
        report(result);
        self.after_history_change();
    }
    fn after_history_change(&mut self) {
        // Renames may have been undone: show the engine's names.
        for track in &self.session.edit().tracks {
            if let Some(view) = self.views.get_mut(&track.id) {
                view.name = track.name.clone();
            }
        }
        self.selected_tracks
            .retain(|id| *id == MASTER_TRACK_ID || self.session.edit().track(*id).is_ok());
        self.prune_clip_selection();
        self.sync_effects();
    }
    /// Whether there is still actions to undo
    pub fn can_undo(&self) -> bool {
        self.session.undo_manager().can_undo()
    }
    /// Whether there is still actions to redo
    pub fn can_redo(&self) -> bool {
        self.session.undo_manager().can_redo()
    }

    fn update_metrics(&mut self) {
        let edit = self.session.edit();
        self.metrics.master.update(edit.master.meter());
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
                .update(channel.meter());
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
                    self.sync_effects();
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
