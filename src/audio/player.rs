use crate::{
    GuiToPlayerMsg, ProcessToGuiMsg,
    audio::{
        clip::ClipBackend,
        engine::Engine,
        export::export_audio,
        metronome::MetronomeBackend,
        preview::PreviewBackend,
        track::{TrackBackend, TrackKind},
    },
    core::{
        message::{AudioToGuiTx, GuiToAudioRx},
        metrics::{AudioMetrics, GlobalMetrics},
        state::{LoopState, PlaybackState},
    },
};
use rtrb::Consumer;
use std::{path::PathBuf, thread, time::Instant};

pub struct PlayerBackend {
    to_gui_tx: AudioToGuiTx,
    from_gui_rx: GuiToAudioRx,
    _midi_rx: Consumer<Vec<u8>>,

    engine: Engine,
    position: f32,
    playhead: usize,

    channels: usize,
    sample_rate: usize,

    preview: PreviewBackend,

    // current state
    playback_state: PlaybackState,
    preview_state: PlaybackState,

    metronome: MetronomeBackend,
    loop_state: LoopState,
}

impl PlayerBackend {
    pub fn new(
        to_gui: AudioToGuiTx,
        from_gui_rx: Consumer<GuiToPlayerMsg>,
        midi_rx: Consumer<Vec<u8>>,
        sample_rate: usize,
    ) -> Self {
        Self {
            from_gui_rx,
            _midi_rx: midi_rx,
            channels: 2,
            playback_state: PlaybackState::Paused,
            sample_rate,
            preview: PreviewBackend::new(),
            preview_state: PlaybackState::Paused,
            metronome: MetronomeBackend::new(),
            engine: Engine::new(sample_rate, 120.),
            position: 0.,
            to_gui_tx: to_gui,
            playhead: 0,
            loop_state: LoopState {
                enabled: false,
                start: 0.,
                end: 16.,
            },
        }
    }

    pub fn mix_audio(&mut self, output: &mut [f32]) {
        // Reset output
        output.fill(0.);
        // Start timer
        let time_start = Instant::now();
        let _ = self.handle_messages();
        let mut metrics = GlobalMetrics::new();
        let num_frames = output.len() / self.channels;

        // Preview
        if self.preview_state == PlaybackState::Playing {
            if let Some(data) = self.preview.read(num_frames, self.sample_rate) {
                for ch in 0..2 {
                    for (i, sample) in data[ch].iter().enumerate() {
                        output[2 * i + ch] = *sample;
                    }
                }
                if let Some(stream) = &self.preview.stream {
                    let _ = self
                        .to_gui_tx
                        .send(ProcessToGuiMsg::PreviewPos(stream.playhead()));
                }
            }
        }

        // Paused
        if self.playback_state == PlaybackState::Paused {
            metrics.processing_ratio =
                time_start.elapsed().as_secs_f32() / (num_frames as f32 / self.sample_rate as f32);

            for (track_id, _) in self.engine.tracks.iter() {
                metrics.tracks.insert(track_id.clone(), AudioMetrics::new());
            }
            metrics.tracks.insert("master".into(), AudioMetrics::new());
            let _ = self.to_gui_tx.send(ProcessToGuiMsg::Metrics(metrics));
            return;
        }

        // Process audio
        self.engine.process(self.playhead, num_frames, &mut metrics);

        // Assign master output buffer

        if let Some(track) = self.engine.tracks.get("master") {
            output.copy_from_slice(track.mix.as_slice());
        }

        // Send data
        metrics.processing_ratio =
            time_start.elapsed().as_secs_f32() / (num_frames as f32 / self.sample_rate as f32);

        let _ = self.to_gui_tx.send(ProcessToGuiMsg::Metrics(metrics));
        let _ = self.to_gui_tx.send(ProcessToGuiMsg::PlaybackPos(
            self.engine.bpm * (self.playhead as f32) / (self.sample_rate as f32 * 60.),
        ));
        // Update playhead
        self.position += self.engine.bpm * num_frames as f32 / (self.sample_rate as f32 * 60.);
        self.playhead += num_frames;

        if self.metronome.enabled && self.playback_state == PlaybackState::Playing {
            self.metronome.render(
                output,
                num_frames,
                self.sample_rate,
                self.playhead,
                self.engine.bpm,
            );
        }
    }

    fn export(&self, path: PathBuf) {
        let engine = self.engine.clone();
        let total_frames = engine.sample_rate * 60;
        let sender_clone = self.to_gui_tx.clone();
        thread::spawn(move || export_audio(engine, total_frames, path, sender_clone));
    }

    fn handle_messages(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        while let Ok(msg) = self.from_gui_rx.pop() {
            if cfg!(debug_assertions) {
                println!("\x1b[1m\x1b[34mOutput Thread: {:?}\x1b[0m", msg);
            }
            match msg {
                GuiToPlayerMsg::Play => {
                    self.playback_state = PlaybackState::Playing;
                    self.preview_state = PlaybackState::Paused;
                }
                GuiToPlayerMsg::Pause => {
                    self.playback_state = PlaybackState::Paused;
                }
                GuiToPlayerMsg::SeekTo(position) => {
                    self.position = position;
                    self.playhead = (position * 60. / self.engine.bpm
                        * self.engine.sample_rate as f32)
                        .floor() as usize;
                }
                GuiToPlayerMsg::AddTrack(id) => {
                    let track = TrackBackend::new_audio_track(&id);
                    // Insert in parent children
                    if let Some(track) = self.engine.tracks.get_mut("master")
                        && let TrackKind::Bus(data) = &mut track.kind
                    {
                        data.children.push(id.clone());
                    }
                    self.engine.tracks.insert(id, track);
                }
                GuiToPlayerMsg::AddClip(
                    track_id,
                    file_path,
                    position,
                    clip_id,
                    trim_start,
                    trim_end,
                ) => {
                    let track = self.engine.tracks.get_mut(&track_id);
                    // Find the track by ID and add a sample to it
                    if let Some(track) = track
                        && let TrackKind::Audio(data) = &mut track.kind
                    {
                        data.clips.push(ClipBackend::new(
                            clip_id, file_path, position, trim_start, trim_end,
                        ));
                    }
                }
                GuiToPlayerMsg::AddClips(map) => {
                    for (track_id, clips) in map {
                        if let Some(track) = self.engine.tracks.get_mut(&track_id)
                            && let TrackKind::Audio(data) = &mut track.kind
                        {
                            for clip in clips {
                                data.clips.push(ClipBackend::from_clipcore(&clip));
                            }
                        }
                    }
                }
                GuiToPlayerMsg::RemoveClip(ids) => {
                    for (_, track) in self.engine.tracks.iter_mut() {
                        if let TrackKind::Audio(data) = &mut track.kind {
                            data.clips.retain(|clip| !ids.contains(&clip.id));
                        }
                    }
                }
                GuiToPlayerMsg::MoveClip(clip_id, track_id, position) => {
                    let mut previous_clip = None;
                    for (_, track) in self.engine.tracks.iter_mut() {
                        let clip = track.remove_clip(clip_id.clone());
                        if let Some(clip) = clip {
                            previous_clip = Some(clip);
                            break;
                        }
                    }

                    let new_track = self.engine.tracks.get_mut(&track_id);

                    if let Some(track) = new_track
                        && let Some(clip) = previous_clip.as_mut()
                        && let TrackKind::Audio(data) = &mut track.kind
                    {
                        clip.pos = position;
                        let clone = clip.clone();
                        data.clips.push(clone);
                    }
                }
                GuiToPlayerMsg::MuteTrack(track_id, value) => {
                    if let Some(track) = self.engine.tracks.get_mut(&track_id) {
                        track.muted = value;
                    }
                }
                GuiToPlayerMsg::ChangeTrackVolume(track_id, value) => {
                    if let Some(track) = self.engine.tracks.get_mut(&track_id) {
                        track.volume = value;
                    }
                }
                GuiToPlayerMsg::ResizeClip(clip_id, trim_start, trim_end, position) => {
                    for (_, track) in self.engine.tracks.iter_mut() {
                        if let TrackKind::Audio(data) = &mut track.kind
                            && let Some(clip) =
                                data.clips.iter_mut().find(|clip| clip.id == clip_id)
                        {
                            clip.trim_start = trim_start;
                            clip.trim_end = trim_end;
                            clip.pos = position;
                            break;
                        }
                    }
                }
                GuiToPlayerMsg::SoloTracks(tracks) => {
                    self.engine.solo_tracks = tracks;
                }
                GuiToPlayerMsg::RemoveTrack(id) => {
                    self.engine.tracks.remove(&id);
                    // Remove from parent children
                    if let Some(track) = self.engine.tracks.get_mut("master")
                        && let TrackKind::Bus(data) = &mut track.kind
                    {
                        data.children.retain(|cid| *cid != id);
                    }
                    self.engine.solo_tracks.retain(|solo| *solo != *id);
                }
                GuiToPlayerMsg::PlayPreview(file) => {
                    if self.playback_state == PlaybackState::Paused {
                        self.preview.play(file);
                        self.preview_state = PlaybackState::Playing
                    }
                }
                GuiToPlayerMsg::PausePreview() => self.preview_state = PlaybackState::Paused,
                GuiToPlayerMsg::SeekPreview(pos) => {
                    self.preview.seek(pos);
                    self.preview_state = PlaybackState::Playing
                }
                GuiToPlayerMsg::UpdateBPM(bpm) => {
                    self.playhead = (self.playhead as f32 * self.engine.bpm / bpm).floor() as usize;
                    self.engine.bpm = bpm;
                }
                GuiToPlayerMsg::AddNode(track_id, index, effect_id, node) => {
                    if let Some(track) = self.engine.tracks.get_mut(&track_id) {
                        track.add_node(effect_id, node, index);
                    }
                }
                GuiToPlayerMsg::RemoveNode(track_id, effect_id) => {
                    if let Some(track) = self.engine.tracks.get_mut(&track_id) {
                        track.remove_node(effect_id);
                    }
                }
                GuiToPlayerMsg::SetNodeEnabled(track_id, effect_id, enabled) => {
                    if let Some(track) = self.engine.tracks.get_mut(&track_id) {
                        track.set_node_enabled(effect_id, enabled);
                    }
                }
                GuiToPlayerMsg::ResizeClips { track_id, clips } => {
                    if let Some(track) = self.engine.tracks.get_mut(&track_id)
                        && let TrackKind::Audio(data) = &mut track.kind
                    {
                        for clip in data.clips.iter_mut() {
                            if let Some((start, end)) = clips.get(&clip.id) {
                                clip.trim_start = *start;
                                clip.trim_end = *end;
                            }
                        }
                    }
                }
                GuiToPlayerMsg::DuplicateTrack {
                    id,
                    new_id,
                    clip_map,
                } => {
                    let Some(track) = self.engine.tracks.get(&id) else {
                        return Ok(());
                    };
                    let new_track = track.duplicate(&new_id, clip_map);
                    // Insert in parent children
                    if let Some(track) = self.engine.tracks.get_mut("master")
                        && let TrackKind::Bus(data) = &mut track.kind
                    {
                        data.children.push(new_id.clone());
                    }
                    self.engine.tracks.insert(new_id, new_track);
                }
                GuiToPlayerMsg::ToggleMetronome(value) => {
                    self.metronome.enabled = value;
                }
                GuiToPlayerMsg::Export(path) => {
                    self.export(path);
                }
                GuiToPlayerMsg::UpdateLoop(new_loop) => {
                    self.loop_state = new_loop;
                }
            }
        }
        Ok(())
    }
}
