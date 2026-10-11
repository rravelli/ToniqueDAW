//! The audio callback and the edit-to-graph path.
//!
//! Means only: for worst-case block times against the deadline, run
//! `cargo run --release -p tonique-engine --example stress`.

mod common;

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use tonique_engine::edit::builder::build_graph;
use tonique_engine::graph::{CompileOptions, compile};

use common::{SAMPLE_RATE, busy_edit, config, playing};

/// One callback at different block sizes and worker counts.
fn process_block(c: &mut Criterion) {
    let mut group = c.benchmark_group("process_block/16_tracks");
    for workers in [0, 2, 4] {
        for block in [64, 256, 1024] {
            let (_session, mut processor) = playing(16, config(block, workers));
            let mut out = vec![0.0; block * 2];
            group.throughput(Throughput::Elements(block as u64));
            group.bench_with_input(
                BenchmarkId::new(format!("{workers}_workers"), block),
                &block,
                |b, _| b.iter(|| processor.process_interleaved(black_box(&mut out), 2)),
            );
        }
    }
    group.finish();
}

/// How the callback scales with the session size, at the default block.
fn track_scaling(c: &mut Criterion) {
    const BLOCK: usize = 256;
    let mut group = c.benchmark_group("process_block/256_frames");
    for workers in [0, 4] {
        for tracks in [8, 32, 128] {
            let (_session, mut processor) = playing(tracks, config(BLOCK, workers));
            let mut out = vec![0.0; BLOCK * 2];
            group.throughput(Throughput::Elements(tracks as u64));
            group.bench_with_input(
                BenchmarkId::new(format!("{workers}_workers"), tracks),
                &tracks,
                |b, _| b.iter(|| processor.process_interleaved(black_box(&mut out), 2)),
            );
        }
    }
    group.finish();
}

/// Rebuilding the graph, which every edit does: building the description,
/// then compiling it.
fn rebuild(c: &mut Criterion) {
    let opts = CompileOptions {
        sample_rate: SAMPLE_RATE,
        max_block: 512,
    };
    let mut group = c.benchmark_group("rebuild");
    for tracks in [8, 32, 128, 256, 512] {
        let edit = busy_edit(tracks);
        group.bench_with_input(BenchmarkId::new("build_graph", tracks), &edit, |b, edit| {
            b.iter(|| build_graph(black_box(edit), SAMPLE_RATE))
        });
        group.bench_with_input(BenchmarkId::new("compile", tracks), &edit, |b, edit| {
            b.iter_batched(
                || build_graph(edit, SAMPLE_RATE),
                |desc| compile(desc, &opts).expect("the fixture compiles"),
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

criterion_group!(benches, process_block, track_scaling, rebuild);
criterion_main!(benches);
