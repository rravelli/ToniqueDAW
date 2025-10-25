use crate::{
    GuiToPlayerMsg, ProcessToGuiMsg,
    audio::{
        clip::ClipBackend,
        preview::PreviewBackend,
        track::{TrackBackend, TrackKind, audio::AudioTrackData},
    },
    core::metrics::{AudioMetrics, GlobalMetrics},
    ui::workspace::PlaybackState,
};
use fundsp::{
    hacker::{AudioUnit, envelope, lowpass_hz},
    hacker32::sine_hz,
};
use rayon::prelude::*;
use rtrb::{Consumer, Producer};
use std::{collections::HashMap, time::Instant};

pub struct PlayerBackend {
    to_gui_tx: Producer<ProcessToGuiMsg>,
    from_gui_rx: Consumer<GuiToPlayerMsg>,
    midi_rx: Consumer<Vec<u8>>,

    channels: usize,
    sample_rate: usize,

    playhead: usize,
    preview: PreviewBackend,

    // current state
    playback_state: PlaybackState,
    preview_state: PlaybackState,
    tracks: HashMap<String, TrackBackend>,
    bpm: f32,
    solo_tracks: Vec<String>,

    _notes: HashMap<u8, Box<dyn AudioUnit>>,
}

pub fn piano_voice(freq: f32, velocity: f32) -> impl AudioUnit {
    // ADSR envelope
    let env = envelope(|t| {
        if t < 0.01 {
            // Attack
            t / 0.01
        } else {
            // Exponential decay
            (-(t - 0.01) * 5.0).exp()
        }
    }) * velocity;

    // Harmonic-rich oscillator
    let osc = sine_hz(freq)                  // fundamental
        + 0.5 * sine_hz(freq * 2.0)          // octave
        + 0.3 * sine_hz(freq * 3.0)          // fifth above octave
        + 0.2 * sine_hz(freq * 4.0) // second octave
         + 0.05 * sine_hz(freq * 2.01)
         + 0.05 * sine_hz(freq * 2.02); // second octave

    // Apply envelope to osc
    (osc * env) >> lowpass_hz(2000.0, 1.0)
    // mellow filter
}

impl PlayerBackend {
    pub fn new(
        to_gui_tx: Producer<ProcessToGuiMsg>,
        from_gui_rx: Consumer<GuiToPlayerMsg>,
        midi_rx: Consumer<Vec<u8>>,
        sample_rate: usize,
    ) -> Self {
        Self {
            to_gui_tx,
            from_gui_rx,
            midi_rx,
            channels: 2,
            playhead: 0,
            bpm: 120.,
            playback_state: PlaybackState::Paused,
            sample_rate,
            tracks: HashMap::new(),
            solo_tracks: vec![],
            preview: PreviewBackend::new(),
            preview_state: PlaybackState::Paused,
            _notes: HashMap::new(),
        }
    }

    pub fn mix_audio(&mut self, output: &mut [f32]) {
        // Reset output
        output.fill(0.);
        // Start timer
        let time_start = Instant::now();
        let _ = self.handle_messages();
        let pos = self.playhead;
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
                        .push(ProcessToGuiMsg::PreviewPos(stream.playhead()));
                }
            }
        }

        while let Ok(e) = self.midi_rx.pop() {
            if e.len() >= 3 {
                let event_type = e[0];
                let note = e[1];
                let vel = e[2];
                let freq = 440.0 * 2f32.powf((note as f32 - 69.0) / 12.0);
                let velocity = 0.3;

                match event_type {
                    // Note On
                    0x90 => {
                        if vel == 0 {
                            self._notes.remove(&note);
                        } else {
                            let mut unit = piano_voice(freq, velocity);
                            unit.set_sample_rate(self.sample_rate as f64);
                            self._notes.insert(note, Box::new(unit));
                        }

                        println!("Playing note freq {}Hz", freq);
                    }
                    // Note Off
                    0x80 => {
                        self._notes.remove(&note);
                    }
                    _ => {}
                }
            }
        }

        for frame in 0..num_frames {
            for (_, unit) in self._notes.iter_mut() {
                let s = unit.get_mono();

                output[2 * frame] += s;
                output[2 * frame + 1] += s;
            }
        }

        // Paused
        if self.playback_state == PlaybackState::Paused {
            metrics.latency =
                time_start.elapsed().as_secs_f32() / (num_frames as f32 / self.sample_rate as f32);

            for (track_id, _) in self.tracks.iter() {
                metrics.tracks.insert(track_id.clone(), AudioMetrics::new());
            }
            metrics.tracks.insert("master".into(), AudioMetrics::new());
            let _ = self.to_gui_tx.push(ProcessToGuiMsg::Metrics(metrics));
            return;
        }
        let mut master_mix = vec![0.; output.len()];

        // Collect tracks
        let tracks: Vec<_> = self.tracks.values_mut().collect();

        tracks.into_par_iter().for_each(|track| {
            let mut track_metrics = AudioMetrics::new();

            track.process(pos, num_frames, self.sample_rate);

            // Compute metrics
            for i in 0..track.mix.len() {
                track_metrics.add_sample(track.mix[i], (i % 2 == 0).into());
            }
        });

        for track in self.tracks.values() {
            if !track.disabled(&self.solo_tracks) {
                for i in 0..track.mix.len() {
                    master_mix[i] += track.mix[i];
                }
            }
            metrics
                .tracks
                .insert(track.id.clone(), track.metrics.clone());
        }

        // Assign master output buffer
        for (i, sample) in output.iter_mut().enumerate() {
            *sample += master_mix[i];
            metrics
                .master
                .add_sample(master_mix[i], (i % 2 == 0).into());
        }
        metrics
            .tracks
            .insert("master".into(), metrics.master.clone());

        // Send data
        metrics.latency =
            time_start.elapsed().as_secs_f32() / (num_frames as f32 / self.sample_rate as f32);

        let _ = self.to_gui_tx.push(ProcessToGuiMsg::Metrics(metrics));
        let _ = self.to_gui_tx.push(ProcessToGuiMsg::PlaybackPos(
            self.bpm * (self.playhead as f32) / (self.sample_rate as f32 * 60.),
        ));
        // Update playhead
        self.playhead += num_frames;
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
                    let frames = (position / self.bpm * 60. * self.sample_rate as f32) as usize;
                    self.playhead = frames;
                }
                GuiToPlayerMsg::AddTrack(id) => {
                    let track =
                        TrackBackend::new(id.clone(), 1.0, TrackKind::Audio(AudioTrackData::new()));

                    self.tracks.insert(id, track);
                }
                GuiToPlayerMsg::AddClip(
                    track_id,
                    file_path,
                    position,
                    clip_id,
                    trim_start,
                    trim_end,
                ) => {
                    let track = self.tracks.get_mut(&track_id);
                    // Find the track by ID and add a sample to it
                    if let Some(track) = track
                        && let TrackKind::Audio(data) = &mut track.kind
                    {
                        data.clips.push(ClipBackend::new(
                            clip_id,
                            file_path,
                            (position / self.bpm * 60. * self.sample_rate as f32).floor() as usize,
                            trim_start,
                            trim_end,
                        ));
                    }
                }
                GuiToPlayerMsg::AddClips(map) => {
                    for (track_id, clips) in map {
                        if let Some(track) = self.tracks.get_mut(&track_id)
                            && let TrackKind::Audio(data) = &mut track.kind
                        {
                            for clip in clips {
                                data.clips.push(ClipBackend::from_clipcore(
                                    &clip,
                                    self.bpm,
                                    self.sample_rate,
                                ));
                            }
                        }
                    }
                }
                GuiToPlayerMsg::RemoveClip(ids) => {
                    for (_, track) in self.tracks.iter_mut() {
                        if let TrackKind::Audio(data) = &mut track.kind {
                            data.clips.retain(|clip| !ids.contains(&clip.id));
                        }
                    }
                }
                GuiToPlayerMsg::MoveClip(clip_id, track_id, position) => {
                    let mut previous_clip = None;
                    for (_, track) in self.tracks.iter_mut() {
                        let clip = track.remove_clip(clip_id.clone());
                        if let Some(clip) = clip {
                            previous_clip = Some(clip);
                            break;
                        }
                    }

                    let new_track = self.tracks.get_mut(&track_id);

                    if let Some(track) = new_track
                        && let Some(clip) = previous_clip.as_mut()
                        && let TrackKind::Audio(data) = &mut track.kind
                    {
                        clip.start_frame =
                            (position / self.bpm * 60. * self.sample_rate as f32).floor() as usize;

                        let clone = clip.clone();

                        data.clips.push(clone);
                    }
                }
                GuiToPlayerMsg::MuteTrack(track_id, value) => {
                    if let Some(track) = self.tracks.get_mut(&track_id) {
                        track.muted = value;
                    }
                }
                GuiToPlayerMsg::ChangeTrackVolume(track_id, value) => {
                    if let Some(track) = self.tracks.get_mut(&track_id) {
                        track.volume = value;
                    }
                }
                GuiToPlayerMsg::ResizeClip(clip_id, trim_start, trim_end, position) => {
                    for (_, track) in self.tracks.iter_mut() {
                        if let TrackKind::Audio(data) = &mut track.kind
                            && let Some(clip) =
                                data.clips.iter_mut().find(|clip| clip.id == clip_id)
                        {
                            clip.trim_start = trim_start;
                            clip.trim_end = trim_end;
                            clip.start_frame = (position / self.bpm * 60. * self.sample_rate as f32)
                                .floor() as usize;
                            break;
                        }
                    }
                }
                GuiToPlayerMsg::SoloTracks(tracks) => {
                    self.solo_tracks = tracks;
                }
                GuiToPlayerMsg::RemoveTrack(id) => {
                    self.tracks.remove(&id);
                    self.solo_tracks.retain(|solo| *solo != *id);
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
                    self.bpm = bpm;
                }
                GuiToPlayerMsg::AddNode(track_id, index, effect_id, node) => {
                    if let Some(track) = self.tracks.get_mut(&track_id) {
                        track.add_node(effect_id, node, index);
                    }
                }
                GuiToPlayerMsg::RemoveNode(track_id, effect_id) => {
                    if let Some(track) = self.tracks.get_mut(&track_id) {
                        track.remove_node(effect_id);
                    }
                }
                GuiToPlayerMsg::SetNodeEnabled(track_id, effect_id, enabled) => {
                    if let Some(track) = self.tracks.get_mut(&track_id) {
                        track.set_node_enabled(effect_id, enabled);
                    }
                }
                GuiToPlayerMsg::ResizeClips { track_id, clips } => {
                    if let Some(track) = self.tracks.get_mut(&track_id)
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
                    let Some(track) = self.tracks.get(&id) else {
                        return Ok(());
                    };
                    let new_track = track.duplicate(&new_id, clip_map);
                    self.tracks.insert(new_id, new_track);
                }
            }
        }
        Ok(())
    }
}
