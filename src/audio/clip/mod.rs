pub mod midi;

use std::path::PathBuf;

use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType};

use crate::{analysis::AudioInfo, cache::AUDIO_ANALYSIS_CACHE, message::CreateClipCommand};

const RESAMPLER_CHUNK_SIZE: usize = 1024;

/// Clip struct for the audio thread.
pub struct ClipBackend {
    pub id: String,
    pub audio: AudioInfo,
    pub start_frame: usize,
    pub trim_start: f32,
    pub trim_end: f32,

    resampler: SincFixedIn<f32>,
    /// Buffer used for the resampler
    input_buffer: Vec<Vec<f32>>,
    /// Frames leftover from the resampler
    resampler_cache_buffer: [Vec<f32>; 2],
    /// Output buffer when resampling
    resampler_output_buffer: [Vec<f32>; 2],
    /// Current playhead inside in the data
    playhead: usize,
    /// Current playhead in the timeline
    timeline_playhead: usize,
}

impl ClipBackend {
    pub fn new(
        id: String,
        path: PathBuf,
        start_frame: usize,
        trim_start: f32,
        trim_end: f32,
    ) -> Self {
        let audio = AUDIO_ANALYSIS_CACHE.get_or_analyze(path.clone()).unwrap();
        let input_buffer = vec![Vec::new(), Vec::new()];
        let output_buffer = [Vec::new(), Vec::new()];
        let resampler = SincFixedIn::<f32>::new(
            1.,
            10.,
            SincInterpolationParameters {
                sinc_len: 16,
                f_cutoff: 0.70,
                oversampling_factor: 8,
                interpolation: SincInterpolationType::Nearest,
                window: rubato::WindowFunction::Hann,
            },
            RESAMPLER_CHUNK_SIZE,
            2,
        )
        .expect("Failed to create resampler");
        let resampler_cache_buffer = [Vec::new(), Vec::new()];

        Self {
            id,
            audio,
            start_frame,
            trim_start: trim_start,
            trim_end: trim_end,
            input_buffer,
            resampler_cache_buffer,
            playhead: 0,
            timeline_playhead: 0,
            resampler,
            resampler_output_buffer: output_buffer,
        }
    }

    /// Reset resampler and buffers
    fn reset(&mut self) {
        self.resampler.reset();
        self.resampler_cache_buffer[0].clear();
        self.resampler_cache_buffer[1].clear();
    }

    /// Resample the clip at given *position* and given *target_size* to given *sample_rate*. The *resampler_output_buffer* is updated by this function. Uses the clip resampler and buffers extra generated frames.
    /// TODO: take into account resampler delay
    fn resample(&mut self, pos: usize, start_index: usize, sample_rate: usize, target_size: usize) {
        // Fill with zero and resize to target size
        self.resampler_output_buffer[0].fill(0.);
        self.resampler_output_buffer[0].resize(target_size, 0.);
        self.resampler_output_buffer[1].fill(0.);
        self.resampler_output_buffer[1].resize(target_size, 0.);

        let end_index = self.playhead_end();
        let chunk_size = self.resampler.input_frames_next();

        let buffer_len = self.resampler_cache_buffer[0].len();
        let mut output_size = buffer_len;

        if pos > self.timeline_playhead || pos + buffer_len < self.timeline_playhead {
            // Reset if resampling to a new position
            self.reset();
            self.playhead = start_index;
            self.timeline_playhead = pos;
            output_size = 0;
        } else if buffer_len > 0 {
            // Copy proper range of the buffer
            let min_range = pos.saturating_sub(self.timeline_playhead);
            let max_range = buffer_len.min(target_size + min_range);
            self.resampler_output_buffer[0][..(max_range - min_range)]
                .copy_from_slice(&self.resampler_cache_buffer[0][min_range..max_range]);
            self.resampler_output_buffer[1][..(max_range - min_range)]
                .copy_from_slice(&self.resampler_cache_buffer[1][min_range..max_range]);

            self.resampler_cache_buffer[0].drain(min_range..max_range);
            self.resampler_cache_buffer[1].drain(min_range..max_range);
        }
        // Update resampler sample rate ratio
        let _ = self
            .resampler
            .set_resample_ratio(sample_rate as f64 / self.audio.sample_rate as f64, false);

        self.input_buffer[0].resize(chunk_size, 0.);
        self.input_buffer[1].resize(chunk_size, 0.);

        while output_size < target_size {
            let chunk_size = self.resampler.input_frames_next();
            let input_size = chunk_size.min(end_index.saturating_sub(self.playhead));
            // Copy data to input buffer
            if let Ok(data) = self.audio.data.lock() {
                self.input_buffer[0].resize(chunk_size, 0.);
                self.input_buffer[1].resize(chunk_size, 0.);

                self.input_buffer[0][..input_size].copy_from_slice(
                    &data.0[self.playhead..(self.playhead + input_size).min(data.0.len())],
                );
                if self.audio.channels > 1 {
                    self.input_buffer[1][..input_size].copy_from_slice(
                        &data.1[self.playhead..(self.playhead + input_size).min(data.0.len())],
                    );
                } else {
                    self.input_buffer[1][..input_size].copy_from_slice(
                        &data.0[self.playhead..(self.playhead + input_size).min(data.0.len())],
                    );
                }
            }
            // Process input
            let res = if input_size < chunk_size {
                self.resampler
                    .process_partial(Some(&self.input_buffer), None)
            } else {
                self.resampler.process(&self.input_buffer, None)
            };

            match res {
                Ok(resampled) => {
                    let remaining_frames = target_size - output_size;
                    let resampled_size = resampled[0].len();
                    self.timeline_playhead += resampled_size;
                    if resampled_size > remaining_frames {
                        self.resampler_output_buffer[0]
                            [output_size..(output_size + remaining_frames)]
                            .copy_from_slice(&resampled[0][..remaining_frames]);
                        self.resampler_output_buffer[1]
                            [output_size..(output_size + remaining_frames)]
                            .copy_from_slice(&resampled[1][..remaining_frames]);

                        self.resampler_cache_buffer[0]
                            .extend_from_slice(&resampled[0][remaining_frames..]);
                        self.resampler_cache_buffer[1]
                            .extend_from_slice(&resampled[1][remaining_frames..]);
                        self.playhead += chunk_size;
                        break;
                    } else {
                        self.resampler_output_buffer[0]
                            [output_size..(output_size + resampled_size)]
                            .copy_from_slice(&resampled[0]);
                        self.resampler_output_buffer[1]
                            [output_size..(output_size + resampled_size)]
                            .copy_from_slice(&resampled[1]);
                    }

                    output_size += resampled_size;
                }
                Err(_) => {
                    break;
                }
            }
            self.playhead += chunk_size;
        }
    }

    /// Get samples for this clip at given position and sample_rate and adds it to the mix_buffer.
    /// The audio is resampled if needed.
    pub fn render_block(
        &mut self,
        mix: &mut Vec<f32>,
        playhead: usize,
        num_frames: usize,
        sample_rate: usize,
    ) {
        // global start position
        let clip_start = self.start_frame;
        // global end position
        let clip_end = self.end(sample_rate);

        // start position of the clip in [pos, pos + num_frames]
        let start = clip_start.max(playhead);
        // end position of the clip in [pos, pos + num_frames]
        let end = clip_end.min(playhead + num_frames);
        // ratio in sample rates
        let sample_rate_ratio = self.audio.sample_rate as f64 / sample_rate as f64;
        // position relative to the clip
        let clip_playhead = self.playhead_start()
            + ((start - clip_start) as f64 * sample_rate_ratio).floor() as usize;

        // start index to pick inside the clip
        let start_index = clip_playhead;
        // Offsets for input buffer
        let start_offset = ((start - playhead) as f64 * sample_rate_ratio).floor() as usize;
        let end_offset = ((end - playhead) as f64 * sample_rate_ratio).floor() as usize;

        if sample_rate as u32 == self.audio.sample_rate {
            if let Ok(data) = self.audio.data.lock() {
                let frames = end_offset - start_offset;
                let data_start = start_index;
                let data_end = data_start + frames;

                let left = &data.0[data_start..data_end];
                let right = if self.audio.channels > 1 {
                    &data.1[data_start..data_end]
                } else {
                    &data.0[data_start..data_end]
                };

                let out_slice = &mut mix[start_offset * 2..end_offset * 2];

                for ((frame, &l), &r) in out_slice
                    .chunks_exact_mut(2)
                    .zip(left.iter())
                    .zip(right.iter())
                {
                    frame[0] += l;
                    frame[1] += r;
                }
            }
            return;
        }
        let frames = end - start;
        self.resample(playhead, clip_playhead, sample_rate, frames);
        // Add samples to mix
        let out_slice = &mut mix[(start - playhead) * 2..(end - playhead) * 2];

        for (frame, (&l, &r)) in out_slice.chunks_exact_mut(2).zip(
            self.resampler_output_buffer[0][0..frames]
                .iter()
                .zip(&self.resampler_output_buffer[1][0..frames]),
        ) {
            frame[0] += l;
            frame[1] += r;
        }
    }

    pub fn num_frames(&self) -> usize {
        self.playhead_end().saturating_sub(self.playhead_start()) as usize
    }

    pub fn playhead_start(&self) -> usize {
        (self.trim_start * self.audio.num_samples.unwrap() as f32).round() as usize
    }

    pub fn playhead_end(&self) -> usize {
        (self.trim_end * self.audio.num_samples.unwrap() as f32).round() as usize
    }

    pub fn end(&self, sample_rate: usize) -> usize {
        self.start_frame
            + (self.num_frames() as f32 * sample_rate as f32 / self.audio.sample_rate as f32)
                .floor() as usize
    }

    pub fn from_command(command: &CreateClipCommand, bpm: f32, sample_rate: usize) -> Self {
        Self::new(
            command.clip_id.clone(),
            command.file_path.clone(),
            (command.position / bpm * 60. * sample_rate as f32).floor() as usize,
            command.trim_start,
            command.trim_end,
        )
    }
}

impl Clone for ClipBackend {
    fn clone(&self) -> Self {
        Self::new(
            self.id.clone(),
            self.audio.path.clone(),
            self.start_frame,
            self.trim_start,
            self.trim_end,
        )
    }
}
