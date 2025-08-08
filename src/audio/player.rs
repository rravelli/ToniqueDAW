use rtrb::{Consumer, Producer};
use std::{collections::HashMap, time::Instant};

use crate::{
    ProcessToGuiMsg,
    audio::{clip::ClipBackend, preview::PreviewBackend, track::TrackBackend},
    components::workspace::PlaybackState,
    message::GuiToPlayerMsg,
    metrics::{AudioMetrics, GlobalMetrics},
};

pub struct PlayerBackend {
    to_gui_tx: Producer<ProcessToGuiMsg>,
    from_gui_rx: Consumer<GuiToPlayerMsg>,
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
}

impl PlayerBackend {
    pub fn new(
        to_gui_tx: Producer<ProcessToGuiMsg>,
        from_gui_rx: Consumer<GuiToPlayerMsg>,
        sample_rate: usize,
    ) -> Self {
        Self {
            to_gui_tx,
            from_gui_rx,
            channels: 2,
            playhead: 0,
            bpm: 120.,
            playback_state: PlaybackState::Paused,
            sample_rate,
            tracks: HashMap::new(),
            solo_tracks: vec![],
            preview: PreviewBackend::new(),
            preview_state: PlaybackState::Paused,
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
                let num_channels = data.len();
                for ch in 0..num_channels {
                    for (i, sample) in data[ch].iter().enumerate() {
                        output[2 * i + ch] = *sample;
                        if num_channels == 1 {
                            output[2 * i + 1] = *sample;
                        }
                    }
                }

                if let Some(stream) = &self.preview.stream {
                    let _ = self
                        .to_gui_tx
                        .push(ProcessToGuiMsg::PreviewPos(stream.playhead()));
                }
            }
        }

        // Paused
        if self.playback_state == PlaybackState::Paused {
            metrics.latency =
                time_start.elapsed().as_secs_f32() / (num_frames as f32 / self.sample_rate as f32);

            for (track_id, _) in self.tracks.iter() {
                metrics.tracks.insert(track_id.clone(), AudioMetrics::new());
            }
            let _ = self.to_gui_tx.push(ProcessToGuiMsg::Metrics(metrics));
            return;
        }
        let mut master_mix = vec![0.; output.len()];

        for (track_id, track) in self.tracks.iter_mut() {
            let disabled = (track.muted && !self.solo_tracks.contains(&track_id))
                || (!self.solo_tracks.is_empty() && !self.solo_tracks.contains(&track_id));

            let track_mix = track.process(pos, num_frames, self.sample_rate);
            // Update master
            let mut track_metrics = AudioMetrics::new();

            for i in 0..track_mix.len() {
                if !disabled {
                    master_mix[i] += track_mix[i];
                }
                track_metrics.add_sample(track_mix[i], (i % 2 == 0).into());
            }
            // Update metrics
            metrics.tracks.insert(track_id.to_string(), track_metrics);
        }

        // Assign master output buffer
        for (i, sample) in output.iter_mut().enumerate() {
            *sample += master_mix[i];
            metrics
                .master
                .add_sample(master_mix[i], (i % 2 == 0).into());
        }

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
                    let track = TrackBackend::new(id.clone(), 1.0);
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
                    if let Some(track) = track {
                        track.clips.push(ClipBackend::new(
                            clip_id,
                            file_path,
                            (position / self.bpm * 60. * self.sample_rate as f32).floor() as usize,
                            trim_start,
                            trim_end,
                        ));
                    }
                }
                GuiToPlayerMsg::RemoveClip(ids) => {
                    for (_, track) in self.tracks.iter_mut() {
                        track.clips.retain(|clip| !ids.contains(&clip.id));
                    }
                }
                // Move clip to new track and new position
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
                    {
                        clip.start_frame =
                            (position / self.bpm * 60. * self.sample_rate as f32).floor() as usize;

                        let clone = clip.clone();

                        //  if in range to be played seek to position
                        if !(self.playhead + 3000 < clip.start_frame
                            || clip.start_frame + clip.stream.info().num_frames
                                < self.playhead + 3000)
                        {}

                        track.clips.push(clone);
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
                GuiToPlayerMsg::ResizeClip(clip_id, trim_start, trim_end) => {
                    for (_, track) in self.tracks.iter_mut() {
                        if let Some(clip) = track.clips.iter_mut().find(|clip| clip.id == clip_id) {
                            clip.trim_start = trim_start;
                            clip.trim_end = trim_end;
                            break;
                        }
                    }
                }
                GuiToPlayerMsg::SoloTracks(tracks) => {
                    self.solo_tracks = tracks;
                }
                GuiToPlayerMsg::RemoveTrack(id) => {
                    self.tracks.remove(&id);
                }
                // Preview
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
            }
        }
        Ok(())
    }
}
