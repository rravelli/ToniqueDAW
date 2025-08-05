use dashmap::DashMap;
use fundsp::{
    MAX_BUFFER_SIZE,
    hacker::{AudioUnit, BufferArray, Fade, NetBackend, pass},
    hacker32::U2,
    net::{Net, NodeId},
};
use rubato::{Resampler, SincFixedOut, SincInterpolationParameters, SincInterpolationType};

use crate::audio::clip::ClipBackend;

pub struct TrackBackend {
    pub id: String,
    pub volume: f32,
    // Sorted by start position for quick access
    pub clips: Vec<ClipBackend>,
    pub muted: bool,
    pub net: Net,

    backend: NetBackend,
    id_hash: DashMap<String, NodeId>,
    units: DashMap<NodeId, Box<dyn AudioUnit>>,
    node_order: Vec<NodeId>,
}

impl TrackBackend {
    pub fn new(id: String, volume: f32) -> Self {
        let mut net = Net::new(2, 2);
        // init network
        net.pass_through(0, 0);
        net.pass_through(1, 1);
        // create backend
        let backend = net.backend();

        TrackBackend {
            id,
            volume,
            clips: Vec::new(),
            muted: false,
            backend,
            net,
            id_hash: DashMap::new(),
            units: DashMap::new(),
            node_order: Vec::new(),
        }
    }

    pub fn process(&mut self, pos: usize, num_frames: usize, sample_rate: usize) -> Vec<f32> {
        let mut mix = vec![0.; num_frames * 2];
        for clip in self.clips.iter_mut() {
            // global start position
            let clip_start = clip.start_frame;
            // global end position
            let clip_end = clip.end(sample_rate);
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
            let sample_rate_ratio = clip.audio.sample_rate as f64 / sample_rate as f64;
            // start index to pick inside the clip
            let start_index = (clip_playhead as f64 * sample_rate_ratio).floor() as usize;
            // end index to pick inside the clip
            let end_index =
                ((clip_playhead + (end - start)) as f64 * sample_rate_ratio).ceil() as usize;

            // compute audio
            if let Ok(data) = clip.audio.data.lock()
                && start_index < end_index
            {
                let mut resampler = SincFixedOut::<f32>::new(
                    sample_rate as f64 / clip.audio.sample_rate as f64,
                    10.,
                    SincInterpolationParameters {
                        sinc_len: 16,
                        f_cutoff: 0.70,
                        oversampling_factor: 8,
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
                if sample_rate != clip.audio.sample_rate as usize {
                    match resampler.process(&channels, None) {
                        Ok(resampled) => {
                            channels = resampled;
                        }
                        Err(_) => {}
                    }
                }

                let mut j = 0;

                for i in (start - pos)..(end - pos) {
                    if j < channels[0].len() {
                        if channels.len() > 1 {
                            mix[2 * i] = channels[0][j] * self.volume;
                            mix[2 * i + 1] += channels[1][j] * self.volume;
                        } else {
                            mix[2 * i] += channels[0][j] * self.volume;
                            mix[2 * i + 1] += channels[0][j] * self.volume;
                        }
                    }
                    j += 1;
                }
            };
        }
        self.process_effects(&mut mix);
        mix
    }

    fn process_effects(&mut self, mix: &mut Vec<f32>) {
        let mut input = BufferArray::<U2>::new();
        let mut output = BufferArray::<U2>::new();
        // Create chunks of MAX_BUFFER_SIZE per channel
        for (chunk_index, chunk) in mix.clone().chunks(2 * MAX_BUFFER_SIZE).enumerate() {
            let size = chunk.len() / 2;
            for (i, s) in chunk.iter().enumerate() {
                input.set_f32(i % 2, i / 2, *s);
            }
            // process effects
            self.backend
                .process(size, &input.buffer_ref(), &mut output.buffer_mut());
            // copy the values
            for i in 0..size {
                mix[2 * chunk_index * MAX_BUFFER_SIZE + 2 * i] = output.at_f32(0, i);
                mix[2 * chunk_index * MAX_BUFFER_SIZE + 2 * i + 1] = output.at_f32(1, i);
            }
        }
    }

    pub fn remove_clip(&mut self, id: String) -> Option<ClipBackend> {
        if let Some(i) = self.clips.iter().position(|clip| clip.id == id) {
            return Some(self.clips.remove(i));
        }
        None
    }

    pub fn seek(&mut self, position: usize) {
        for clip in self.clips.iter_mut() {
            if clip.start_frame <= position
                && position <= clip.start_frame + clip.stream.info().num_frames
            {
            } else if clip.start_frame > position {
            }
        }
    }
}
