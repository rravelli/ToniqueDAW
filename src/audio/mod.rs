use crate::{
    audio::{
        clip::ClipBackend,
        engine::Engine,
        export::export_audio,
        input::spawn_input_thread,
        metronome::MetronomeBackend,
        output::{OutputStream, OutputStreamMessage, spawn_output_stream, spawn_output_thread},
        preview::PreviewBackend,
        track::{TrackBackend, TrackKind},
    },
    core::{
        message::{AudioToGuiTx, GuiToAudioRx, GuiToPlayerMsg, ProcessToGuiMsg},
        metrics::{AudioMetrics, GlobalMetrics},
        state::{LoopState, PlaybackState},
    },
};
use cpal::{
    Device, Stream, available_hosts,
    traits::{DeviceTrait, HostTrait},
};
use crossbeam::channel::{Sender, bounded, unbounded};
use rtrb::Consumer;
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::atomic::AtomicU32,
    thread::spawn,
    time::Instant,
};

mod clip;
mod engine;
mod export;
mod input;
mod instrument;
mod metronome;
pub mod midi;
mod output;
pub mod player;
mod preview;
mod process;
mod track;

pub const CHUNK_SIZE: usize = 1024;

struct DeviceStream {
    stream: cpal::Stream,
    subscribers: Vec<Sender<Vec<f32>>>,
}

pub fn spawn_audio_thread(
    to_gui_tx: AudioToGuiTx,
    from_gui_rx: GuiToAudioRx,
    midi_rx: Consumer<Vec<u8>>,
) -> (Stream, Stream) {
    // Record sound from devices
    let input_stream = spawn_input_thread();
    // Play sound to device
    let output_stream = spawn_output_stream(to_gui_tx, from_gui_rx, midi_rx);

    (input_stream, output_stream)
}

pub fn spawn_audio_manager(
    to_gui_tx: AudioToGuiTx,
    from_gui_rx: GuiToAudioRx,
    midi_rx: Consumer<Vec<u8>>,
) {
    spawn(|| {
        let mut manager = AudioManager::new(from_gui_rx, to_gui_tx);
        manager.run_loop();
    });
}

struct AudioManager {
    rx: GuiToAudioRx,
    tx: AudioToGuiTx,
    host: cpal::Host,
    input_streams: HashMap<String, DeviceStream>,
    output_stream: Option<OutputStream>,
    engine: Engine,
    buffer: Vec<f32>,
    playhead: usize,
    metrics: GlobalMetrics,
    loop_state: LoopState,
    playback_state: PlaybackState,
    preview_state: PlaybackState,
    preview: PreviewBackend,
    metronome: MetronomeBackend,
    acc: VecDeque<f32>,
}

impl AudioManager {
    fn new(rx: GuiToAudioRx, tx: AudioToGuiTx) -> Self {
        let mut manager = Self {
            rx,
            tx,
            host: cpal::default_host(),
            input_streams: HashMap::new(),
            output_stream: None,
            engine: Engine::new(44100, 120.),
            buffer: vec![0.; 2 * CHUNK_SIZE],
            acc: VecDeque::new(),
            playhead: 0,
            playback_state: PlaybackState::Paused,
            preview_state: PlaybackState::Paused,
            preview: PreviewBackend::new(),
            metrics: GlobalMetrics::new(),
            loop_state: LoopState {
                enabled: false,
                end: 0.,
                start: 0.,
            },
            metronome: MetronomeBackend::new(),
        };

        if let Some(device) = manager.host.default_output_device() {
            let _ = manager.start_output(&device);
        }

        manager
    }

    pub fn run_loop(&mut self) {
        loop {
            let can_process = self.output_stream.is_some() && self.acc.len() < 7 * CHUNK_SIZE;

            if can_process {
                self.process();
            }

            if let Some(output) = &mut self.output_stream {
                while let Ok(requested_frames) = output.rx.pop() {
                    // Get first frames in acc
                    let frames: Vec<f32> = (0..requested_frames)
                        .filter_map(|_| self.acc.pop_front())
                        .collect();

                    match output
                        .tx
                        .push(OutputStreamMessage::AddChunk(frames.clone()))
                    {
                        Ok(_) => {
                            if self.preview_state == PlaybackState::Playing
                                && let Some(stream) = &self.preview.stream
                            {
                                let _ =
                                    self.tx.send(ProcessToGuiMsg::PreviewPos(stream.playhead()));
                            }
                            if self.playback_state == PlaybackState::Playing {
                                output.playhead += frames.len() / 2;
                            } else {
                                self.metrics.reset();
                            }

                            let _ = self.tx.send(ProcessToGuiMsg::Metrics(self.metrics.clone()));
                        }
                        Err(_) => todo!(),
                    }
                }
            }

            let _ = self.handle_messages();
        }
    }

    pub fn process(&mut self) {
        if self.preview_state == PlaybackState::Paused
            && self.playback_state == PlaybackState::Paused
        {
            return;
        }
        // Start Timer
        let time_start = Instant::now();
        // Reset Buffer
        self.buffer.fill(0.);
        // Start processing
        if self.preview_state == PlaybackState::Playing {
            self.process_preview();
        }
        if self.playback_state == PlaybackState::Playing {
            self.process_master();
        }
        // Update
        self.metrics.processing_ratio = time_start.elapsed().as_secs_f32()
            / (CHUNK_SIZE as f32 / self.engine.sample_rate as f32);

        self.acc.extend(self.buffer.clone());
    }

    fn process_master(&mut self) {
        self.engine
            .process(self.playhead, CHUNK_SIZE, &mut self.metrics);

        if let Some(track) = self.engine.tracks.get("master") {
            self.buffer.copy_from_slice(track.mix.as_slice());
        }

        if self.metronome.enabled {
            self.metronome.render(
                &mut self.buffer,
                CHUNK_SIZE,
                self.engine.sample_rate,
                self.playhead,
                self.engine.bpm,
            );
        }

        self.playhead += CHUNK_SIZE;
    }

    fn process_preview(&mut self) {
        if let Some(data) = self.preview.read(CHUNK_SIZE, self.engine.sample_rate) {
            for ch in 0..2 {
                for (i, sample) in data[ch].iter().enumerate() {
                    self.buffer[2 * i + ch] = *sample;
                }
            }
        }
    }

    fn start_output(self: &mut AudioManager, device: &Device) -> Result<(), String> {
        if self
            .output_stream
            .as_ref()
            .is_some_and(|s| s.name == device.name().unwrap())
        {
            return Ok(());
        }

        self.engine.sample_rate = device
            .default_output_config()
            .map_or(44100, |config| config.sample_rate().0 as usize);

        let (tx, rx) = rtrb::RingBuffer::new(40);
        let (tx2, rx2) = rtrb::RingBuffer::new(40);
        let stream = spawn_output_thread(device.clone(), rx, tx2)?;

        self.output_stream = Some(OutputStream {
            name: device.name().unwrap().to_string(),
            _stream: stream,
            tx,
            rx: rx2,
            playhead: 0,
        });

        let _ = self.tx.send(ProcessToGuiMsg::DeviceChanged(
            device.name().map_or(None, |f| Some(f)),
        ));

        Ok(())
    }

    fn export(&self, path: PathBuf) {
        let engine = self.engine.clone();
        let total_frames = engine.sample_rate * 60;
        let sender_clone = self.tx.clone();
        spawn(move || export_audio(engine, total_frames, path, sender_clone));
    }

    fn handle_messages(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        while let Ok(msg) = self.rx.pop() {
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
                    if let Some(output) = &mut self.output_stream {
                        self.playhead = output.playhead;
                    }
                }
                GuiToPlayerMsg::SeekTo(position) => {
                    self.playhead = (position * 60. / self.engine.bpm
                        * self.engine.sample_rate as f32)
                        .floor() as usize;
                    self.acc.clear();
                    if let Some(output) = &mut self.output_stream {
                        output.playhead = self.playhead;
                    }
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
                    self.acc.clear();
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
