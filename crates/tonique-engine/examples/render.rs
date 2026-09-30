//! Offline render of the demo song:
//! `cargo run --release -p tonique-engine --example render -- out.wav`

mod demo;

use std::time::Instant;

use demo::demo_edit;
use tonique_engine::edit::EditSession;
use tonique_engine::engine::{Engine, EngineConfig, render_offline};
use tonique_engine::sample::WavWriter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "demo.wav".into());
    let config = EngineConfig {
        sample_rate: 48000.0,
        max_block: 256,
        worker_threads: 2,
        ..Default::default()
    };
    let (engine, mut processor) = Engine::new(config);
    let mut session = EditSession::new(demo_edit(config.sample_rate), engine)?;
    let stats = session.last_compile_stats().unwrap();
    println!(
        "graph: {} described, {} scheduled, {} deduplicated, {} latency delays, {} buffers",
        stats.described,
        stats.scheduled,
        stats.deduplicated,
        stats.delays_inserted,
        stats.buffer_slots
    );

    session.play()?;
    let end = session.edit().length();
    let seconds = session.edit().tempo.beats_to_seconds(end) + 2.0; // + tail
    let frames = (seconds * config.sample_rate) as usize;
    let started = Instant::now();
    let audio = render_offline(&mut processor, frames, 2);
    let took = started.elapsed().as_secs_f64();

    let mut wav = WavWriter::create(&path, 2, config.sample_rate)?;
    wav.write_interleaved(&audio)?;
    wav.finalize()?;
    let peak = audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    println!(
        "rendered {seconds:.1}s to {path} in {took:.3}s ({:.0}x real time), peak {peak:.2}",
        seconds / took
    );
    Ok(())
}
