//! New, save and open: turning the state into a [`ProjectFile`] and back.

use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use egui::Vec2;
use tonique_engine::{
    edit::{
        Bus, BusId, Channel, ClipLoop, Edit, MAX_TRANSPOSE, Output, SourceId, StretchSettings,
        Track, TrackId, WarpMode,
        commands::{AddBus, SetBypass, SetOutput, SetSolo},
    },
    time::BeatPos,
};

use super::{
    DEFAULT_BPM, DEFAULT_LOOP_BARS, MASTER_TRACK_ID, ProjectState, report, sources::SourceRegistry,
};
use crate::{
    analysis::AudioInfo,
    cache::AUDIO_ANALYSIS_CACHE,
    core::{
        clip::{AudioClip, MIN_LENGTH},
        effect::{EffectKind, Setting},
        project::{
            ChannelFile, ClipFile, EffectFile, GroupFile, LoopFile, ProjectFile, TrackFile,
            VERSION, resolve_path, store_path,
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
        self.clear_selection_history();
        self.pending_actions.clear();
        self.batching = false;
        self.resized_clip = None;
        self.grid.offset = Vec2::ZERO;
        self.sync_time_signature();
        self.loop_range = (
            BeatPos::ZERO,
            BeatPos(DEFAULT_LOOP_BARS * self.grid.beats_per_bar()),
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
                    clips: self.clip_files(track, dir),
                }
            })
            .collect();
        ProjectFile {
            version: VERSION,
            bpm,
            time_signature: edit.tempo.time_signature.into(),
            loop_range: (self.loop_range.0.0, self.loop_range.1.0),
            looping: self.looping,
            master: self.channel_file(&edit.master),
            groups,
            tracks,
        }
    }

    /// `track`'s clips as saved: in timeline order, so the same clips save
    /// the same however they came about (undo and redo can leave them in
    /// another order), and don't read as unsaved changes.
    fn clip_files(&self, track: &Track, dir: Option<&Path>) -> Vec<ClipFile> {
        let mut clips: Vec<ClipFile> = track
            .clips
            .iter()
            .filter_map(|clip| self.clip_view(clip))
            .map(|clip| ClipFile {
                path: store_path(&clip.audio.path, dir),
                position: clip.position.0,
                offset: clip.offset,
                length: Some(clip.length),
                looped: clip.looped.map(|l| LoopFile {
                    start: l.start_s,
                    end: l.end_s,
                }),
                trim_start: None,
                trim_end: None,
                gain: clip.gain,
                pan: clip.pan,
                origin: clip.origin,
                warp: clip.warp.into(),
                bpm: Some(clip.bpm),
                transpose: clip.pitch(),
                window: clip.stretch.window.into(),
                formants: clip.stretch.formants,
            })
            .collect();
        clips.sort_by(|a, b| a.position.total_cmp(&b.position));
        clips
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
    /// couldn't be restored (unknown parameters); the rest is loaded anyway.
    /// Clips of missing files are kept, silent: see [`Self::missing_files`].
    /// Loading isn't undoable.
    pub fn load_project(&mut self, project: &ProjectFile, dir: &Path) -> Vec<String> {
        self.new_project();
        let mut problems = Vec::new();
        self.set_bpm(project.bpm);
        match project.time_signature.signature() {
            Some(signature) => self.set_time_signature(signature),
            None => problems.push(format!(
                "Unsupported time signature {}/{}: using 4/4",
                project.time_signature.numerator, project.time_signature.denominator
            )),
        }
        self.set_loop_range(BeatPos(project.loop_range.0), BeatPos(project.loop_range.1));
        self.set_looping(project.looping);
        self.restore_channel(MASTER_TRACK_ID, &project.master, &mut problems);
        let bar = self.grid.beats_per_bar();
        let reach = reach_by_file(project, self.bpm(), bar);

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
                .zip(placeholder_lengths(track, bar))
                .filter_map(|(clip, placeholder)| {
                    self.restore_clip(clip, dir, &reach, placeholder, &mut problems)
                })
                .collect();
            self.add_clips(&id, clips);
        }
        // Whatever the file said, keep groups together and never empty.
        self.normalize();
        self.remove_empty_groups();
        self.session.clear_history();
        self.clear_selection_history();
        problems
    }

    fn restore_clip(
        &mut self,
        clip: &ClipFile,
        dir: &Path,
        reach: &HashMap<&Path, f64>,
        placeholder: f64,
        problems: &mut Vec<String>,
    ) -> Option<AudioClip> {
        let path: PathBuf = resolve_path(&clip.path, dir);
        let found = analyze(&path);
        let audio = match (&found, reach.get(clip.path.as_path())) {
            (Some(audio), _) => audio.clone(),
            // Kept as they were, to play once the file is located.
            (None, Some(seconds)) => AudioInfo::missing(path, *seconds),
            (None, None) => {
                problems.push(format!("Missing audio file: {}", clip.path.display()));
                return None;
            }
        };
        let bpm = self.bpm();
        let mut restored = AudioClip::new(self.new_clip_id(), audio, BeatPos(clip.position), bpm);
        restored.gain = clip.gain;
        restored.pan = clip.pan;
        restored.origin = clip.origin;
        restored.warp = clip.warp.into();
        restored.bpm = clip.bpm.filter(|b| *b > 0.).unwrap_or(bpm as f64);
        // In range, whatever a project file says.
        let limit = MAX_TRANSPOSE;
        let transpose = clip.transpose.clamp(-limit, limit);
        restored.transpose = transpose.round() as i32;
        restored.detune = (transpose - transpose.round()) * 100.;
        restored.stretch = StretchSettings {
            window: clip.window.into(),
            formants: clip.formants,
        };
        restored.looped = clip.looped.map(|l| ClipLoop {
            start_s: l.start,
            end_s: l.end,
        });
        match (clip.length, clip.trim_start, clip.trim_end) {
            // Older projects give the part of the file played, which can't
            // be told without the file: from its start, for the
            // placeholder's length.
            (None, _, _) if found.is_none() => restored.length = placeholder,
            (Some(length), _, _) => {
                restored.offset = clip.offset;
                restored.length = length;
            }
            // Older projects: the part of the file played.
            (None, start, end) => {
                let (start, end) = (start.unwrap_or(0.), end.unwrap_or(1.));
                restored.offset = start as f64 * restored.source_seconds() as f64;
                restored.length = (end - start) as f64 * restored.source_beats(bpm);
            }
        }
        Some(restored)
    }

    /// Files used by clips that weren't found, by name: each clip using
    /// one is kept, silent, until it's located.
    pub fn missing_files(&self) -> Vec<(SourceId, &AudioInfo)> {
        self.sources.missing()
    }

    /// Use the file at `path` for the missing file `source`: every clip
    /// using it plays it from now on. Then look for the other missing
    /// files in the same folder, by name. Returns how many files were
    /// located, or why `path` can't be used. Not undoable, but the new
    /// paths are unsaved changes.
    pub fn locate_file(&mut self, source: SourceId, path: &Path) -> Result<usize, String> {
        let audio = analyze(path).ok_or_else(|| {
            format!(
                "“{}” isn't an audio file Tonique can read.",
                path.file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
            )
        })?;
        self.sources.relink(source, audio);
        let others = path.parent().map_or(0, |dir| self.search_files(dir, false));
        Ok(1 + others)
    }

    /// Look for the missing files in `dir` by name, in its subfolders too
    /// when `recursive`, and use those found. Returns how many were.
    pub fn search_files(&mut self, dir: &Path, recursive: bool) -> usize {
        let names: Vec<(SourceId, OsString)> = self
            .missing_files()
            .into_iter()
            .filter_map(|(id, audio)| Some((id, audio.path.file_name()?.to_owned())))
            .collect();
        if names.is_empty() {
            return 0;
        }
        let found = find_files(dir, recursive, |name| {
            names.iter().any(|(_, missing)| missing == name)
        });
        let mut located = 0;
        for (id, name) in names {
            let audio = found
                .iter()
                .filter(|path| path.file_name() == Some(name.as_os_str()))
                .find_map(|path| analyze(path));
            if let Some(audio) = audio {
                self.sources.relink(id, audio);
                located += 1;
            }
        }
        located
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

/// The file at `path`, if it's there and of known length. The cache
/// remembers files by path, even ones deleted since: check first.
fn analyze(path: &Path) -> Option<AudioInfo> {
    path.is_file()
        .then(|| AUDIO_ANALYSIS_CACHE.get_or_analyze(path.to_path_buf()))
        .flatten()
        .filter(|audio| audio.duration.is_some())
}

/// How far into each file (as stored) the project's clips read, in
/// seconds at `bpm`: the least a missing file is known to last. Clips of
/// older projects count from the file's start, at their placeholder's
/// length.
fn reach_by_file(project: &ProjectFile, bpm: f32, bar: f64) -> HashMap<&Path, f64> {
    let mut reach: HashMap<&Path, f64> = HashMap::new();
    let clips = project
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter().zip(placeholder_lengths(track, bar)));
    for (clip, placeholder) in clips {
        let (offset, length) = match clip.length {
            Some(length) => (clip.offset, length),
            None => (0., placeholder),
        };
        // Played faster or slower, the clip reads more or less of its file.
        let file_bpm = clip.bpm.filter(|b| *b > 0.).unwrap_or(bpm as f64);
        let warp = WarpMode::from(clip.warp).play(
            file_bpm,
            clip.transpose,
            StretchSettings::default(),
            bpm as f64,
        );
        let end = offset + length * 60. / bpm as f64 * warp.speed();
        let read = match clip.looped {
            // Round the loop rather than on through the file.
            Some(looped) if offset < looped.end => looped.end,
            _ => end,
        };
        let longest = reach.entry(clip.path.as_path()).or_default();
        *longest = longest.max(read);
    }
    reach
}

/// Files in `dir` (and its subfolders when `recursive`, skipping hidden
/// ones) whose name is `wanted`.
fn find_files(dir: &Path, recursive: bool, wanted: impl Fn(&OsStr) -> bool) -> Vec<PathBuf> {
    /// Enough for a sample library, not a whole disk.
    const MAX_ENTRIES: usize = 100_000;
    let mut found = Vec::new();
    let mut folders = vec![dir.to_path_buf()];
    let mut seen = 0;
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > MAX_ENTRIES {
                return found;
            }
            let name = entry.file_name();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if recursive && !name.to_string_lossy().starts_with('.') {
                    folders.push(entry.path());
                }
            } else if wanted(&name) {
                found.push(entry.path());
            }
        }
    }
    found
}

/// For each of `track`'s clips, the length to show it at when its part of
/// its file isn't known (older projects, file missing): up to the next
/// clip, at most a `bar`, so it overlaps none.
fn placeholder_lengths(track: &TrackFile, bar: f64) -> Vec<f64> {
    track
        .clips
        .iter()
        .map(|clip| {
            let next = track
                .clips
                .iter()
                .map(|other| other.position)
                .filter(|position| *position > clip.position)
                .fold(f64::INFINITY, f64::min);
            (next - clip.position).min(bar).max(MIN_LENGTH)
        })
        .collect()
}
