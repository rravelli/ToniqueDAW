//! Offline and UI-side processing: resampling, waveform peaks, spectrum.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use tonique_engine::audio::AudioBuffer;
use tonique_engine::meter::ChannelMeter;
use tonique_engine::peaks::WaveformPeaks;
use tonique_engine::resample::resample_offline;
use tonique_engine::sample::SampleBuffer;
use tonique_engine::spectrum::Spectrum;

fn sine(len: usize, rate: f64) -> Vec<f32> {
    (0..len)
        .map(|i| (std::f64::consts::TAU * 440.0 * i as f64 / rate).sin() as f32)
        .collect()
}

/// Importing a 10 s file at a foreign rate (one channel).
fn resample(c: &mut Criterion) {
    let mut group = c.benchmark_group("resample_offline/10s");
    for (from, to) in [(44100.0, 48000.0), (96000.0, 48000.0)] {
        let input = sine(from as usize * 10, from);
        group.throughput(Throughput::Elements(input.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{from}->{to}")),
            &input,
            |b, input| b.iter(|| resample_offline(black_box(input), from, to)),
        );
    }
    group.finish();
}

/// Building the peak pyramid of a 3 minute stereo file, then drawing it.
fn peaks(c: &mut Criterion) {
    const RATE: f64 = 48000.0;
    let channel = sine(RATE as usize * 180, RATE);
    let buffer = SampleBuffer::new(vec![channel.clone(), channel], RATE);
    let mut group = c.benchmark_group("peaks/3min_stereo");
    group.throughput(Throughput::Elements(buffer.len() as u64));
    group.bench_function("from_buffer", |b| {
        b.iter(|| WaveformPeaks::from_buffer(black_box(&buffer)))
    });
    group.throughput(Throughput::Elements(1));

    // One frame of a 1000 px wide waveform, both channels, at several zooms.
    let peaks = WaveformPeaks::from_buffer(&buffer);
    const COLUMNS: usize = 1000;
    for visible in [buffer.len(), buffer.len() / 64, COLUMNS * 4] {
        let per_column = visible / COLUMNS;
        group.bench_with_input(
            BenchmarkId::new("draw_1000_columns", visible),
            &per_column,
            |b, &per_column| {
                b.iter(|| {
                    for ch in 0..2 {
                        for col in 0..COLUMNS {
                            let start = col * per_column;
                            black_box(peaks.range_approx(
                                ch,
                                start,
                                start + per_column,
                                (per_column / 4).max(1),
                            ));
                        }
                    }
                })
            },
        );
    }
    let bytes = peaks.to_bytes();
    group.bench_function("to_bytes", |b| b.iter(|| black_box(&peaks).to_bytes()));
    group.bench_function("from_bytes", |b| {
        b.iter(|| WaveformPeaks::from_bytes(black_box(&bytes)))
    });
    group.finish();
}

/// One analysis of the master spectrum (the analyser's per-frame cost when
/// it is due).
fn spectrum(c: &mut Criterion) {
    let meter = ChannelMeter::default();
    let mut buffer = AudioBuffer::new(2, 4096);
    for ch in 0..2 {
        buffer.channel_mut(ch).copy_from_slice(&sine(4096, 48000.0));
    }
    meter.record(&buffer.block(4096));
    let mut spectrum = Spectrum::new();
    // A `dt` of a whole second makes every update analyse.
    c.bench_function("spectrum/update", |b| {
        b.iter(|| spectrum.update(black_box(&meter), 48000.0, 1.0))
    });
}

criterion_group!(benches, resample, peaks, spectrum);
criterion_main!(benches);
