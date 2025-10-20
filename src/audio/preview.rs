use std::path::PathBuf;

use creek::{ReadDiskStream, ReadStreamOptions, SymphoniaDecoder};
use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType};

const RESAMPLER_CHUNK_SIZE: usize = 1024;

pub struct PreviewBackend {
    pub stream: Option<Box<ReadDiskStream<SymphoniaDecoder>>>,
    file: Option<PathBuf>,
    resampler: SincFixedIn<f32>,
    buffer: Vec<Vec<f32>>,
}

impl PreviewBackend {
    pub fn new() -> Self {
        let buffer = vec![Vec::new(), Vec::new()];
        Self {
            stream: None,
            file: None,
            resampler: SincFixedIn::<f32>::new(
                1.,
                10.,
                SincInterpolationParameters {
                    sinc_len: 256,
                    f_cutoff: 0.95,
                    oversampling_factor: 8,
                    interpolation: SincInterpolationType::Nearest,
                    window: rubato::WindowFunction::Hann,
                },
                RESAMPLER_CHUNK_SIZE,
                2,
            )
            .unwrap(),
            buffer,
        }
    }

    fn reset(&mut self) {
        self.buffer[0].clear();
        self.buffer[1].clear();
        self.resampler.reset();
    }

    pub fn seek(&mut self, pos: usize) {
        if let Some(stream) = &mut self.stream {
            let _ = stream.seek(pos, creek::SeekMode::Auto);
            self.reset();
        }
    }

    pub fn play(&mut self, file: PathBuf) {
        self.reset();
        if self.file.clone().is_some_and(|f| f == file) {
            if let Some(stream) = &mut self.stream {
                let _ = stream.seek(0, creek::SeekMode::Auto);
            }
            return;
        }
        let mut stream =
            Box::new(ReadDiskStream::new(file, 0, ReadStreamOptions::default()).unwrap());
        let _ = stream.cache(0, 0);
        let _ = stream.seek(0, creek::SeekMode::Auto);
        self.stream = Some(stream);
    }

    fn resample(&mut self, num_frames: usize, sample_rate: usize) -> Option<[Vec<f32>; 2]> {
        if let Some(stream) = &mut self.stream
            && stream.playhead() < stream.info().num_frames
            && stream.is_ready().unwrap()
        {
            let _ = self.resampler.set_resample_ratio(
                sample_rate as f64 / stream.info().sample_rate.unwrap() as f64,
                false,
            );

            let mut output = [vec![0.; num_frames], vec![0.; num_frames]];

            let buffer_len = self.buffer[0].len();
            let mut output_len = buffer_len;
            if buffer_len > 0 {
                // Copy proper range of the buffer
                let min_range = 0;
                let max_range = buffer_len.min(num_frames + min_range);
                output[0][..(max_range - min_range)]
                    .copy_from_slice(&self.buffer[0][min_range..max_range]);
                output[1][..(max_range - min_range)]
                    .copy_from_slice(&self.buffer[1][min_range..max_range]);
                // Remove copied samples from the buffer
                self.buffer[0].drain(min_range..max_range);
                self.buffer[1].drain(min_range..max_range);
            }

            let input_frames = self.resampler.input_frames_next();

            while stream.info().num_frames - stream.playhead() > 0 && output_len < num_frames {
                let data = stream
                    .read(input_frames.min(stream.info().num_frames - stream.playhead()))
                    .unwrap();
                let input = if data.num_channels() > 1 {
                    &[data.read_channel(0), data.read_channel(1)]
                } else {
                    &[data.read_channel(0), data.read_channel(0)]
                };

                let res = if data.num_frames() == num_frames {
                    self.resampler.process(input, None)
                } else {
                    self.resampler.process_partial(Some(input), None)
                };

                match res {
                    Ok(mut resampled) => {
                        if resampled[0].len() > num_frames - output_len {
                            let remaining_frames = num_frames - output_len;
                            output[0][output_len..]
                                .copy_from_slice(&resampled[0][..remaining_frames]);
                            output[1][output_len..]
                                .copy_from_slice(&resampled[1][..remaining_frames]);

                            self.buffer[0] = resampled[0][remaining_frames..].to_vec();
                            self.buffer[1] = resampled[1][remaining_frames..].to_vec();
                            break;
                        } else {
                            output[0][output_len..(output_len + resampled[0].len())]
                                .copy_from_slice(&mut resampled[0]);
                            output[1][output_len..(output_len + resampled[0].len())]
                                .copy_from_slice(&mut resampled[1]);
                        }
                        output_len += resampled[0].len();
                    }
                    Err(err) => {
                        println!("Error while resampling {}", err);
                        break;
                    }
                }
            }

            return Some(output);
        }
        None
    }

    /// Return the next chunk of samples of size *num_frames* for the preview
    pub fn read(&mut self, num_frames: usize, sample_rate: usize) -> Option<[Vec<f32>; 2]> {
        if let Some(stream) = &mut self.stream
            && stream.playhead() < stream.info().num_frames
            && stream.is_ready().unwrap()
        {
            let audio_sample_rate = stream.info().sample_rate.unwrap() as usize;
            // Same sample rate
            if audio_sample_rate == sample_rate {
                let data = stream.read(num_frames).unwrap();
                if data.num_channels() > 1 {
                    return Some([data.read_channel(0).to_vec(), data.read_channel(1).to_vec()]);
                } else {
                    return Some([data.read_channel(0).to_vec(), data.read_channel(0).to_vec()]);
                }
            }
        }
        return self.resample(num_frames, sample_rate);
    }
}
