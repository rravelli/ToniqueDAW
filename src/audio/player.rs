use rtrb::{Consumer, Producer};
use std::collections::HashMap;

use crate::{
    ProcessToGuiMsg,
    audio::{clip::ClipBackend, track::TrackBackend},
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

    // current state
    playback_state: PlaybackState,
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
        }
    }

    pub fn mix_audio(&mut self, output: &mut [f32]) {
        let _ = self.handle_messages();
        let pos = self.playhead;
        let mut metrics = GlobalMetrics::new();

        // Paused
        if self.playback_state == PlaybackState::Paused {
            for sample in output {
                *sample = 0.;
            }
            let _ = self.to_gui_tx.push(ProcessToGuiMsg::Metrics(metrics));
            return;
        }
        let num_frames = output.len() / self.channels;
        let mut master_mix = vec![0.; output.len()];

        for (track_id, track) in self.tracks.iter_mut() {
            let mut track_mix = vec![0.; output.len()];
            let disabled = (track.muted && !self.solo_tracks.contains(&track_id))
                || (!self.solo_tracks.is_empty() && !self.solo_tracks.contains(&track_id));

            for clip in track.clips.iter_mut() {
                let clip_start = clip.start_frame;
                let clip_end = clip.end();
                // not in range
                if pos + num_frames < clip_start || clip_end < pos + num_frames {
                    continue;
                }
                // not ready
                if let Ok(ready) = clip.audio.ready.lock()
                    && !*ready
                {
                    continue;
                };

                let start = clip_start.max(pos);
                let end = clip_end.min(pos + num_frames);

                let clip_playhead = clip.playhead_start() + (start - clip_start);

                let index_start = clip_playhead;
                let index_end = clip_playhead + (end - start);
                // compute audio
                if let Ok(data) = clip.audio.data.lock()
                    && index_end > index_start
                {
                    let mut channels = vec![];

                    channels.push(&data.0[clip_playhead..clip_playhead + (end - start)]);
                    if clip.audio.is_stereo {
                        channels.push(&data.1[clip_playhead..clip_playhead + (end - start)]);
                    }

                    let mut j = 0;
                    for i in (start - pos)..(end - pos) {
                        if channels.len() > 1 {
                            track_mix[2 * i] += channels[0][j] * track.volume;
                            track_mix[2 * i + 1] += channels[1][j] * track.volume;
                        } else {
                            track_mix[2 * i] += channels[0][j] * track.volume;
                            track_mix[2 * i + 1] += channels[0][j] * track.volume;
                        }
                        j += 1;
                    }
                };
            }
            // Update master
            let mut track_metrics = AudioMetrics::new();

            for i in 0..track_mix.len() {
                if !disabled {
                    master_mix[i] += track_mix[i];
                }
                track_metrics.add_sample(track_mix[i], (i % 2 == 0).into());
            }
            //  Update metrics
            metrics.tracks.insert(track_id.to_string(), track_metrics);
        }

        for (i, sample) in output.iter_mut().enumerate() {
            *sample = master_mix[i];
            metrics
                .master
                .add_sample(master_mix[i], (i % 2 == 0).into());
        }
        let _ = self.to_gui_tx.push(ProcessToGuiMsg::Metrics(metrics));

        self.playhead += num_frames;
        let _ = self
            .to_gui_tx
            .push(ProcessToGuiMsg::PlaybackPos(self.bpm * (self.playhead as f32) / (self.sample_rate as f32 * 60.)));
    }

    fn handle_messages(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        while let Ok(msg) = self.from_gui_rx.pop() {
            match msg {
                GuiToPlayerMsg::Play => {
                    self.playback_state = PlaybackState::Playing;
                }
                GuiToPlayerMsg::Pause => {
                    self.playback_state = PlaybackState::Paused;
                }
                GuiToPlayerMsg::SeekTo(position) => {
                    let frames = (position / self.bpm * 60. * self.sample_rate as f32) as usize;
                    for (_, track) in self.tracks.iter_mut() {
                        track.seek(frames);
                    }
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
                GuiToPlayerMsg::RemoveClip(clip_id, track_id) => {
                    let track = self.tracks.get_mut(&track_id);
                    if let Some(track) = track {
                        track.remove_clip(clip_id);
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

                        let mut clone = clip.clone();
                        let mut seek_position = 0;
                        //  if in range to be played seek to position
                        if !(self.playhead + 3000 < clip.start_frame
                            || clip.start_frame + clip.stream.info().num_frames
                                < self.playhead + 3000)
                        {
                            let start = clip.start_frame.max(self.playhead);
                            seek_position = start - clip.start_frame;
                        }

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
            }
        }
        Ok(())
    }
}

// TO BE DELETED
fn resample_linear(input: &[f32], src_rate: usize, dst_rate: usize) -> Vec<f32> {
    if src_rate == dst_rate {
        return input.to_vec();
    }

    let ratio = dst_rate as f64 / src_rate as f64;
    let mut output = Vec::with_capacity((input.len() as f64 * ratio) as usize);

    for i in 0..((input.len() as f64 * ratio) as usize) {
        let src_idx = i as f64 / ratio;
        let idx = src_idx.floor() as usize;
        let frac = src_idx - idx as f64;

        let a = input.get(idx).copied().unwrap_or(0.0);
        let b = input.get(idx + 1).copied().unwrap_or(a);

        output.push((1.0 - frac) as f32 * a + frac as f32 * b);
    }

    output
}
