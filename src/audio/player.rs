use rtrb::{Consumer, Producer};
use rubato::{Resampler, SincFixedOut, SincInterpolationParameters, SincInterpolationType};
use std::{collections::HashMap, time::Instant};

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
        let time_start = Instant::now();
        let _ = self.handle_messages();
        let pos = self.playhead;
        let mut metrics = GlobalMetrics::new();

        // Paused
        if self.playback_state == PlaybackState::Paused {
            output.fill(0.);
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
                // global start position
                let clip_start = clip.start_frame;
                // global end position
                let clip_end = clip.end(self.sample_rate);
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
                // start position of the clip in [pos, pos + num_frames]
                let start = clip_start.max(pos);
                // end position of the clip in [pos, pos + num_frames]
                let end = clip_end.min(pos + num_frames);
                // position relative to the clip
                let clip_playhead = clip.playhead_start() + (start - clip_start);
                // ratio in sample rates
                let sample_rate_ratio = clip.audio.sample_rate as f64 / self.sample_rate as f64;
                // start index to pick inside the clip
                let start_index = (clip_playhead as f64 * sample_rate_ratio).floor() as usize;
                // end index to pick inside the clip
                let end_index =
                    ((clip_playhead + (end - start)) as f64 * sample_rate_ratio).ceil() as usize;

                // compute audio
                if let Ok(data) = clip.audio.data.lock()
                    && start_index > end_index
                {
                    let mut resampler = SincFixedOut::new(
                        self.sample_rate as f64 / clip.audio.sample_rate as f64,
                        10.,
                        SincInterpolationParameters {
                            sinc_len: 32,
                            f_cutoff: 0.70,
                            oversampling_factor: 16,
                            interpolation: SincInterpolationType::Nearest,
                            window: rubato::WindowFunction::Hann,
                        },
                        end - pos,
                        2,
                    )
                    .unwrap();

                    let mut channels = vec![];

                    channels.push(
                        data.0[start_index.min(data.0.len() - 1)
                            ..(start_index + resampler.input_frames_next()).min(data.0.len())]
                            .to_vec(),
                    );
                    if clip.audio.is_stereo {
                        channels.push(
                            data.1[start_index.min(data.1.len() - 1)
                                ..(end_index + resampler.input_frames_next()).min(data.1.len())]
                                .to_vec(),
                        );
                    }
                    // resample if needed
                    if self.sample_rate != clip.audio.sample_rate as usize {
                        match resampler.process(&channels, None) {
                            Ok(resampled) => {
                                channels = resampled;
                            }
                            Err(_) => {}
                        }
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
            // Update metrics
            metrics.tracks.insert(track_id.to_string(), track_metrics);
        }
        // Assign master output buffer
        for (i, sample) in output.iter_mut().enumerate() {
            *sample = master_mix[i];
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
            }
        }
        Ok(())
    }
}
