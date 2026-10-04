//! New, save and open: turning the state into a [`ProjectFile`] and back.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use egui::Vec2;
use tonique_engine::{
    edit::{
        Bus, BusId, Channel, Edit, Output, TrackId,
        commands::{AddBus, SetBypass, SetOutput, SetSolo},
    },
    time::BeatPos,
};

use super::{
    DEFAULT_BPM, DEFAULT_LOOP_BARS, MASTER_TRACK_ID, ProjectState, report, sources::SourceRegistry,
};
use crate::{
    cache::AUDIO_ANALYSIS_CACHE,
    core::{
        clip::AudioClip,
        effect::{EffectKind, Setting},
        project::{
            ChannelFile, ClipFile, EffectFile, GroupFile, ProjectFile, TrackFile, VERSION,
            resolve_path, store_path,
        },
        track::TrackView,
    },
    utils::color::{format_color, parse_color},
};

impl ProjectState {
    /// Start an empty project on the same engine: no tracks, default tempo
    /// and loop, empty undo history. Settings, the audio output and view
    /// preferences (panels, theme palette, metronome switch) are kept.
    pub fn new_project(&mut self) {
        self.stop();
        self.pause_preview();
        report(self.session.reset(Edit::new(DEFAULT_BPM)));

        let mut master = TrackView::new();
        master.name = "Master".into();
        self.views = HashMap::from([(MASTER_TRACK_ID, master)]);
        self.collapsed_effects.clear();
        self.sources = SourceRegistry::new(self.session.engine().config().sample_rate);
        self.playhead = BeatPos::ZERO;
        self.edit_cursor = BeatPos::ZERO;
        self.selected_tracks.clear();
        self.clip_selection = Default::default();
        self.pending_actions.clear();
        self.batching = false;
        self.resized_clip = None;
        self.grid.offset = Vec2::ZERO;
        self.loop_range = (
            BeatPos::ZERO,
            BeatPos(DEFAULT_LOOP_BARS * self.grid.beats_per_bar() as f64),
        );
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
        // Groups in the order they're met, so parents come first.
        let mut order: Vec<BusId> = Vec::new();
        for track in &edit.tracks {
            for bus in edit.buses_along(track.output).into_iter().rev() {
                if !order.contains(&bus) {
                    order.push(bus);
                }
            }
        }
        let index_of = |output: Output| match output {
            Output::Bus(bus) => order.iter().position(|b| *b == bus),
            Output::Master => None,
        };
        let groups = order
            .iter()
            .filter_map(|bus| edit.bus(*bus).ok())
            .map(|group| {
                let id = TrackId(group.id.0);
                let view = self.views.get(&id).cloned().unwrap_or_else(TrackView::new);
                GroupFile {
                    name: group.name.clone(),
                    color: format_color(view.color),
                    height: view.height,
                    collapsed: view.collapsed,
                    soloed: group.soloed,
                    parent: index_of(group.output),
                    channel: self.channel_file(&group.channel),
                }
            })
            .collect();
        let tracks = edit
            .tracks
            .iter()
            .map(|track| {
                let view = self
                    .views
                    .get(&track.id)
                    .cloned()
                    .unwrap_or_else(TrackView::new);
                TrackFile {
                    name: track.name.clone(),
                    color: format_color(view.color),
                    height: view.height,
                    collapsed: view.collapsed,
                    soloed: track.soloed,
                    group: index_of(track.output),
                    channel: self.channel_file(&track.channel),
                    clips: track
                        .clips
                        .iter()
                        .filter_map(|clip| self.clip_view(clip, bpm))
                        .map(|clip| ClipFile {
                            path: store_path(&clip.audio.path, dir),
                            position: clip.position.0,
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
            loop_range: (self.loop_range.0.0, self.loop_range.1.0),
            looping: self.looping,
            master: self.channel_file(&edit.master),
            groups,
            tracks,
        }
    }

    fn channel_file(&self, channel: &Channel) -> ChannelFile {
        ChannelFile {
            volume: channel.volume.get(),
            pan: channel.pan.get(),
            muted: channel.muted,
            effects: channel
                .plugins
                .iter()
                .filter_map(|plugin| {
                    Some(EffectFile {
                        kind: EffectKind::of(&plugin.kind)?,
                        name: plugin.name.clone(),
                        enabled: !plugin.bypassed,
                        collapsed: self.collapsed_effects.contains(&plugin.id),
                        params: plugin
                            .params
                            .iter()
                            .map(|p| (p.name, p.get()))
                            .chain(Setting::of(&plugin.kind).map(Setting::saved))
                            .map(|(name, value)| (name.to_string(), value))
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
        self.set_loop_range(BeatPos(project.loop_range.0), BeatPos(project.loop_range.1));
        self.set_looping(project.looping);
        self.restore_channel(MASTER_TRACK_ID, &project.master, &mut problems);

        // Parents come first, so each group can go straight into its parent.
        let mut groups: Vec<TrackId> = Vec::new();
        for group in &project.groups {
            let mut bus = self.session.create(|e| Bus::new(e, group.name.clone()));
            let parent = group.parent.and_then(|i| groups.get(i)).copied();
            if let Some(parent) = parent {
                bus.output = Output::Bus(BusId(parent.0));
            }
            let id = TrackId(bus.id.0);
            self.perform(AddBus::new(bus));
            let mut view = TrackView::new();
            view.name.clone_from(&group.name);
            view.height = group.height;
            view.collapsed = group.collapsed;
            if let Some(color) = parse_color(&group.color) {
                view.color = color;
            }
            self.views.insert(id, view);
            if group.soloed {
                self.perform(SetSolo::bus(BusId(id.0), true));
            }
            self.restore_channel(id, &group.channel, &mut problems);
            groups.push(id);
        }

        for track in &project.tracks {
            let id = self.add_track();
            if let Some(group) = track.group.and_then(|i| groups.get(i)) {
                self.perform(SetOutput::new(id, Output::Bus(BusId(group.0))));
            }
            let view = self.track_view_mut(&id);
            view.name.clone_from(&track.name);
            view.height = track.height;
            view.collapsed = track.collapsed;
            if let Some(color) = parse_color(&track.color) {
                view.color = color;
            }
            self.commit_track_view(&id);
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
        // Whatever the file said, keep groups together and never empty.
        self.normalize();
        self.remove_empty_groups();
        self.session.clear_history();
        problems
    }

    fn restore_clip(
        &mut self,
        clip: &ClipFile,
        dir: &Path,
        problems: &mut Vec<String>,
    ) -> Option<AudioClip> {
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
        Some(AudioClip {
            id: self.new_clip_id(),
            audio,
            position: BeatPos(clip.position),
            trim_start: clip.trim_start,
            trim_end: clip.trim_end,
        })
    }

    fn restore_channel(&mut self, id: TrackId, channel: &ChannelFile, problems: &mut Vec<String>) {
        if let Ok(c) = self.edit().channel(self.channel_ref(id)) {
            c.volume.set(channel.volume);
            c.pan.set(channel.pan);
        }
        if channel.muted {
            self.set_mute(id, true);
        }
        for (index, effect) in channel.effects.iter().enumerate() {
            let setting = effect
                .params
                .iter()
                .find_map(|(name, value)| Setting::parse(effect.kind, name, *value));
            self.add_effect_with(&id, effect.kind, setting, index);
            let Some(plugin) = self.effects(&id).get(index).map(|e| e.plugin.clone()) else {
                continue;
            };
            for (name, value) in &effect.params {
                if Setting::parse(effect.kind, name, *value).is_some() {
                    continue;
                }
                match plugin.params.iter().find(|p| p.name == name) {
                    Some(param) => param.set(*value),
                    None => problems.push(format!("Unknown effect parameter: {name}")),
                }
            }
            if !effect.enabled {
                self.perform(SetBypass::new(self.channel_ref(id), plugin.id, true));
            }
            if effect.name.is_some() {
                self.rename_effect(&id, plugin.id, effect.name.clone());
            }
            self.set_effect_collapsed(plugin.id, effect.collapsed);
        }
    }
}
