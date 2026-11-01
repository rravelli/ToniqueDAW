//! Worst-case block times against the real-time deadline, which criterion's
//! means hide. Renders a busy session for increasingly many tracks and
//! reports the tail of the per-block times:
//! `cargo run --release -p tonique-engine --example stress -- [block] [workers] [seconds]`

#[path = "../benches/common/mod.rs"]
mod common;

use std::time::Instant;

use common::{SAMPLE_RATE, config, playing};

fn main() {
    let mut args = std::env::args().skip(1).map(|a| a.parse::<usize>());
    let mut arg = |default| args.next().and_then(Result::ok).unwrap_or(default);
    let (block, workers, seconds) = (arg(256), arg(2), arg(20));
    let budget_us = block as f64 / SAMPLE_RATE * 1e6;
    let blocks = (seconds as f64 * SAMPLE_RATE / block as f64) as usize;
    println!(
        "{block} frames at {SAMPLE_RATE} Hz, {workers} workers: {budget_us:.0} µs per block, {blocks} blocks per run"
    );
    println!(
        "{:>6} {:>6} {:>6} {:>9} {:>9} {:>9} {:>9} {:>7}",
        "tracks", "nodes", "slots", "p50 µs", "p99 µs", "p99.9 µs", "max µs", "late"
    );

    for tracks in [8, 16, 32, 64, 128, 256, 512, 1024, 2048] {
        let (session, mut processor) = playing(tracks, config(block, workers));
        let stats = session
            .last_compile_stats()
            .expect("the session compiled the edit");
        let mut out = vec![0.0; block * 2];
        let mut times: Vec<f64> = Vec::with_capacity(blocks);
        for _ in 0..blocks {
            let started = Instant::now();
            processor.process_interleaved(&mut out, 2);
            times.push(started.elapsed().as_secs_f64() * 1e6);
        }
        times.sort_by(f64::total_cmp);
        let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
        let late = times.iter().filter(|&&t| t > budget_us).count();
        println!(
            "{tracks:>6} {:>6} {:>6} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {late:>7}",
            stats.scheduled,
            stats.buffer_slots,
            at(0.5),
            at(0.99),
            at(0.999),
            at(1.0)
        );
        if at(0.999) > budget_us {
            println!("p99.9 is over budget: stopping");
            break;
        }
    }
}
