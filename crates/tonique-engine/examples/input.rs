//! Check an input device: lists the inputs, then prints a device's levels,
//! latencies and dropouts. Nothing is recorded or played back.
//! `cargo run --release -p tonique-engine --example input --features device -- [device id]`

use std::time::Duration;

use tonique_engine::device::{InputDevice, InputOptions, OutputDevice, input_devices};
use tonique_engine::engine::{Engine, EngineConfig};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for device in input_devices() {
        println!("{}  ({})", device.id, device.name);
    }
    let id = std::env::args().nth(1);

    let output = OutputDevice::default_output()?;
    let config = EngineConfig {
        sample_rate: output.sample_rate(),
        max_block: 256,
        ..Default::default()
    };
    let output_frames = output.buffer_frames().unwrap_or(512) as usize;
    let output_id = output.id();
    let (mut engine, processor) = Engine::new(config);
    let _output = output.start(processor)?;

    let input = InputDevice::open(&InputOptions {
        device_id: id,
        engine_rate: config.sample_rate,
        buffer_frames: None,
        output_id,
    })?;
    println!(
        "\n{}: {} channels at {} Hz, {}",
        input.name(),
        input.channels(),
        input.sample_rate(),
        if input.needs_resampling() {
            "resampled"
        } else {
            "on the output's clock"
        },
    );
    let (feed, writer) = input.feed(config.max_block, output_frames);
    let status = feed.status();
    let stream = input.start(writer)?;
    engine.set_input(Some(feed))?;

    while !stream.is_lost() {
        std::thread::sleep(Duration::from_millis(250));
        let peaks: Vec<String> = (0..status.channels())
            .map(|c| format!("{:5.3}", status.take_peak(c)))
            .collect();
        println!(
            "peaks {}  latency in {:5} out {:5}  fill {:5} held {:5}  underruns {} overflows {}",
            peaks.join(" "),
            status.latency_frames(),
            engine.output_latency_frames(),
            status.fill_frames(),
            status.mean_fill_frames(),
            status.underruns(),
            status.overflows(),
        );
    }
    println!("input lost");
    Ok(())
}
