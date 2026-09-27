//! New, save and open: turning the state into a [`ProjectFile`] and back.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use egui::Vec2;
use tonique_engine::edit::{
    Channel, Edit, TrackId,
    commands::{SetBypass, SetSolo},
};

use super::{
    DEFAULT_BPM, DEFAULT_LOOP_BARS, MASTER_TRACK_ID, ToniqueProjectState, report,
    sources::SourceRegistry,
};
use crate::{
    cache::AUDIO_ANALYSIS_CACHE,
    core::{
        clip::ClipCore,
        project::{
            ChannelFile, ClipFile, EffectFile, ProjectFile, TrackFile, VERSION, resolve_path,
            store_path,
        },
        track::MutableTrackCore,
    },
    ui::theme::{format_color, parse_color},
};

impl ToniqueProjectState {
    /// Start an empty project on the same engine: no tracks, default tempo
    /// and loop, empty undo history. Settings, the audio output and view
    /// preferences (panels, theme palette, metronome switch) are kept.
    pub fn new_project(&mut self) {
        self.stop();
        self.pause_preview();
        report(self.session.reset(Edit::new(DEFAULT_BPM)));

        let mut master = MutableTrackCore::new();
        master.name = "Master".into();
        self.views = HashMap::from([(MASTER_TRACK_ID, master)]);
        self.sources = SourceRegistry::new(self.session.engine().config().sample_rate);
        self.playhead = 0.;
        self.edit_cursor = 0.;
        self.selected_tracks.clear();
        self.clip_selection = Default::default();
        self.effects.clear();
        self.detached_effects.clear();
        self.pending_actions.clear();
        self.batching = false;
        self.resized_clip = None;
        self.grid.offset = Vec2::ZERO;
        self.loop_range = (0., DEFAULT_LOOP_BARS * self.grid.beats_per_bar() as f32);
        self.looping = false;
        self.sync_loop();
        self.graph.reset();
        // The metronome's level lives on the edit.
        self.apply_metronome_level();
    }

    /// The project as saved in a file in `dir` (`None` before its first
    /// save: audio paths are then absolute).
    pub fn project(&self, dir: Option<&Path>) -> ProjectFile {
        let edit = self.edit();
        let bpm = self.bpm();
        let tracks = edit
            .tracks
            .iter()
            .map(|track| {
                let view = self
                    .views
                    .get(&track.id)
                    .cloned()
                    .unwrap_or_else(MutableTrackCore::new);
                TrackFile {
                    name: track.name.clone(),
                    color: format_color(view.color),
                    height: view.height,
                    closed: view.closed,
                    soloed: track.soloed,
                    channel: self.channel_file(track.id, &track.channel),
                    clips: track
                        .clips
                        .iter()
                        .filter_map(|clip| self.clip_view(clip, bpm))
                        .map(|clip| ClipFile {
                            path: store_path(&clip.audio.path, dir),
                            position: clip.position,
                            trim_start: clip.trim_start,
                            trim_end: clip.trim_end,
                        })
                        .collect(),
                }
            })
            .collect();
        ProjectFile {
            version: VERSION,
            bpm,
            loop_range: self.loop_range,
            looping: self.looping,
            master: self.channel_file(MASTER_TRACK_ID, &edit.master),
            tracks,
        }
    }

    fn channel_file(&self, track: TrackId, channel: &Channel) -> ChannelFile {
        ChannelFile {
            volume: channel.volume.get(),
            pan: channel.pan.get(),
            muted: channel.muted,
            effects: channel
                .plugins
                .iter()
                .filter_map(|plugin| {
                    Some(EffectFile {
                        kind: self.effect(track, plugin.id)?.effect_id(),
                        enabled: !plugin.bypassed,
                        params: plugin
                            .params
                            .iter()
                            .map(|p| (p.name.to_string(), p.get()))
                            .collect(),
                    })
                })
                .collect(),
        }
    }

    /// Replace the project with `project`, saved in `dir`. Returns what
    /// couldn't be restored (missing audio files, unknown parameters); the
    /// rest is loaded anyway. Loading isn't undoable.
    pub fn load_project(&mut self, project: &ProjectFile, dir: &Path) -> Vec<String> {
        self.new_project();
        let mut problems = Vec::new();
        self.set_bpm(project.bpm);
        self.set_loop_range(project.loop_range.0, project.loop_range.1);
        self.set_looping(project.looping);
        self.restore_channel(MASTER_TRACK_ID, &project.master, &mut problems);

        for track in &project.tracks {
            let id = self.add_track();
            let view = self.track_mut(&id);
            view.name.clone_from(&track.name);
            view.height = track.height;
            view.closed = track.closed;
            if let Some(color) = parse_color(&track.color) {
                view.color = color;
            }
            self.commit_track_mut(&id);
            if track.soloed {
                self.perform(SetSolo::new(id, true));
            }
            self.restore_channel(id, &track.channel, &mut problems);

            let clips = track
                .clips
                .iter()
                .filter_map(|clip| self.restore_clip(clip, dir, &mut problems))
                .collect();
            self.add_clips(&id, clips);
        }
        self.sync_effects();
        self.session.clear_history();
        problems
    }

    fn restore_clip(
        &mut self,
        clip: &ClipFile,
        dir: &Path,
        problems: &mut Vec<String>,
    ) -> Option<ClipCore> {
        let path: PathBuf = resolve_path(&clip.path, dir);
        // The cache remembers files by path, even ones deleted since.
        let audio = path
            .exists()
            .then(|| AUDIO_ANALYSIS_CACHE.get_or_analyze(path))
            .flatten()
            .filter(|audio| audio.duration.is_some());
        let Some(audio) = audio else {
            problems.push(format!("Missing audio file: {}", clip.path.display()));
            return None;
        };
        Some(ClipCore {
            id: self.new_clip_id(),
            audio,
            position: clip.position,
            trim_start: clip.trim_start,
            trim_end: clip.trim_end,
        })
    }

    fn restore_channel(&mut self, id: TrackId, channel: &ChannelFile, problems: &mut Vec<String>) {
        if let Ok(c) = self.edit().channel(Self::channel_ref(id)) {
            c.volume.set(channel.volume);
            c.pan.set(channel.pan);
        }
        if channel.muted {
            self.set_mute(id, true);
        }
        for (index, effect) in channel.effects.iter().enumerate() {
            self.add_effect(&id, effect.kind, index);
            let Some(plugin) = self
                .edit()
                .channel(Self::channel_ref(id))
                .ok()
                .and_then(|c| c.plugins.get(index))
                .cloned()
            else {
                continue;
            };
            for (name, value) in &effect.params {
                match plugin.params.iter().find(|p| p.name == name) {
                    Some(param) => param.set(*value),
                    None => problems.push(format!("Unknown effect parameter: {name}")),
                }
            }
            if !effect.enabled {
                self.perform(SetBypass::new(Self::channel_ref(id), plugin.id, true));
            }
            self.sync_effects();
            if let Some(editor) = self.effects.get_mut(&id).and_then(|e| e.get_mut(index)) {
                editor.read_params();
            }
        }
    }
}
