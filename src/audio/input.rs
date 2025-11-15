use crate::core::metrics::AudioMetrics;
use cpal::{
    BufferSize, Stream,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

pub fn spawn_input_thread() -> Stream {
    let host = cpal::default_host();

    let device = host
        .default_input_device()
        .expect("no input device available");

    let sample_rate = device.default_output_config().unwrap().sample_rate();

    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate,
        buffer_size: BufferSize::Default,
    };

    let stream = device
        .build_input_stream(
            &config,
            |input: &[f32], _| {
                let mut metrics = AudioMetrics::new();
                for (i, sample) in input.iter().enumerate() {
                    metrics.add_sample(*sample, (i % 2 == 0) as usize);
                }
            },
            |err| eprintln!("{}", err),
            None,
        )
        .expect("Failed to create stream");

    let _ = stream.play().unwrap();

    stream
}
