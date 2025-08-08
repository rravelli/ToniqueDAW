use std::path::PathBuf;

use creek::{ReadDiskStream, ReadStreamOptions, SymphoniaDecoder};
use rubato::{Resampler, SincFixedOut, SincInterpolationParameters, SincInterpolationType};

pub struct PreviewBackend {
    pub stream: Option<Box<ReadDiskStream<SymphoniaDecoder>>>,
    file: Option<PathBuf>,
}

impl PreviewBackend {
    pub fn new() -> Self {
        Self {
            stream: None,
            file: None,
        }
    }

    pub fn seek(&mut self, pos: usize) {
        if let Some(stream) = &mut self.stream {
            let _ = stream.seek(pos, creek::SeekMode::Auto);
        }
    }

    pub fn play(&mut self, file: PathBuf) {
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

    pub fn read(&mut self, num_frames: usize, sample_rate: usize) -> Option<Vec<Vec<f32>>> {
        if let Some(stream) = &mut self.stream
            && stream.playhead() < stream.info().num_frames
        {
            let audio_sample_rate = stream.info().sample_rate.unwrap() as usize;
            let num_channels = stream.info().num_channels as usize;
            // Same sample rate
            if audio_sample_rate == sample_rate {
                let data = stream.read(num_frames).unwrap();
                let mut channels = Vec::new();
                channels.push(data.read_channel(0).to_vec());
                // stereo
                if num_channels > 1 {
                    channels.push(data.read_channel(1).to_vec());
                }
                return Some(channels);
            }

            let num_frames_out = num_frames.min(
                (sample_rate as f32 / audio_sample_rate as f32
                    * (stream.info().num_frames - stream.playhead()) as f32)
                    .floor() as usize,
            );

            let mut resampler = SincFixedOut::new(
                sample_rate as f64 / audio_sample_rate as f64,
                10.,
                SincInterpolationParameters {
                    sinc_len: 16,
                    f_cutoff: 0.70,
                    oversampling_factor: 8,
                    interpolation: SincInterpolationType::Nearest,
                    window: rubato::WindowFunction::Hann,
                },
                num_frames_out,
                num_channels,
            )
            .unwrap();

            let data = stream.read(resampler.input_frames_next()).unwrap();

            let mut channels = vec![];
            channels.push(data.read_channel(0));

            if num_channels > 1 {
                channels.push(data.read_channel(1));
            }

            let resampled = resampler.process(&channels, None);

            if let Ok(res) = resampled {
                return Some(res);
            }
        }
        None
    }
}
