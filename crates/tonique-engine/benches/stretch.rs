//! Stretched clips: playing them, and building the graphs that hold them.
//! Needs feature `signalsmith`: `cargo bench -p tonique-engine --features
//! signalsmith --bench stretch`.

mod common;

use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use tonique_engine::edit::{
    Clip, ClipContent, Edit, EditSession, StretchSettings, StretchWindow, Track, WarpMode,
};
use tonique_engine::engine::Engine;
use tonique_engine::sample::SampleBuffer;
use tonique_engine::time::BeatPos;

use common::{LOOP_BEATS, SAMPLE_RATE, config, playing_edit};

/// The edit's tempo; stretched clips' sources are at [`SOURCE_BPM`].
const BPM: f64 = 140.0;
const SOURCE_BPM: f64 = 100.0;

/// How a clip plays its source, from cheapest to dearest.
#[derive(Clone, Copy)]
enum Play {
    /// As it is.
    Off,
    /// Transposed by resampling.
    Repitch,
    /// Stretched to the edit's tempo.
    Stretch,
}

impl Play {
    fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Repitch => "repitch",
            Self::Stretch => "stretch",
        }
    }
}

/// `tracks` tracks of `clips_per_track` stereo clips one after the other,
/// [`LOOP_BEATS`] long each, playing as `play` says.
fn clips_edit(tracks: usize, clips_per_track: usize, play: Play) -> Edit {
    clips_edit_with(
        tracks,
        clips_per_track,
        play,
        StretchSettings::default(),
        0.0,
    )
}

/// As [`clips_edit`], stretched as `settings` say, transposed by
/// `transpose` when stretched.
fn clips_edit_with(
    tracks: usize,
    clips_per_track: usize,
    play: Play,
    settings: StretchSettings,
    semitones: f32,
) -> Edit {
    let mut edit = Edit::new(BPM);
    // Long enough for the fastest clip to play to its end.
    let len = SAMPLE_RATE as usize * 8;
    let sine: Vec<f32> = (0..len).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
    let source = edit.add_source(Arc::new(SampleBuffer::new(vec![sine; 2], SAMPLE_RATE)));
    for i in 0..tracks {
        let mut track = Track::new(&mut edit, format!("t{i}"));
        for c in 0..clips_per_track {
            let start = BeatPos(c as f64 * LOOP_BEATS);
            let mut clip = Clip::audio(&mut edit, start, LOOP_BEATS, source);
            if let ClipContent::Audio {
                warp,
                source_bpm,
                transpose,
                stretch,
                ..
            } = &mut clip.content
            {
                match play {
                    Play::Off => {}
                    Play::Repitch => *transpose = 2.0,
                    Play::Stretch => {
                        *warp = WarpMode::Stretch;
                        *source_bpm = SOURCE_BPM;
                        *transpose = semitones;
                        *stretch = settings;
                    }
                }
            }
            track.clips.push(clip);
        }
        edit.tracks.push(track);
    }
    edit
}

/// One callback with `clips` clips playing at once, one per track, each
/// way they can play.
fn process_block(c: &mut Criterion) {
    const BLOCK: usize = 256;
    let mut group = c.benchmark_group("stretch/process_block/256_frames");
    for play in [Play::Off, Play::Repitch, Play::Stretch] {
        for clips in [1, 8, 32] {
            let (_session, mut processor) =
                playing_edit(clips_edit(clips, 1, play), config(BLOCK, 0));
            let mut out = vec![0.0; BLOCK * 2];
            group.throughput(Throughput::Elements(clips as u64));
            group.bench_with_input(BenchmarkId::new(play.name(), clips), &clips, |b, _| {
                b.iter(|| processor.process_interleaved(black_box(&mut out), 2))
            });
        }
    }
    group.finish();
}

/// One callback with 8 stretched clips playing at once, transposed, with
/// each window, keeping their formants or not.
fn settings(c: &mut Criterion) {
    const BLOCK: usize = 256;
    const CLIPS: usize = 8;
    let mut group = c.benchmark_group("stretch/settings/8_clips_256_frames");
    for window in StretchWindow::ALL {
        for formants in [false, true] {
            let settings = StretchSettings { window, formants };
            let edit = clips_edit_with(CLIPS, 1, Play::Stretch, settings, 3.0);
            let (_session, mut processor) = playing_edit(edit, config(BLOCK, 0));
            let mut out = vec![0.0; BLOCK * 2];
            let name = format!("{window:?}{}", if formants { "+formants" } else { "" });
            group.throughput(Throughput::Elements(CLIPS as u64));
            group.bench_function(name.to_lowercase(), |b| {
                b.iter(|| processor.process_interleaved(black_box(&mut out), 2))
            });
        }
    }
    group.finish();
}

/// Building and loading the graph of 32 stretched clips: first, every clip
/// gets a new stretcher; then, rebuilt over a graph that played, they're
/// handed over.
fn rebuild(c: &mut Criterion) {
    const CLIPS: usize = 32;
    let mut group = c.benchmark_group("stretch/rebuild/32_clips");
    group.sample_size(20);
    group.bench_function("first_build", |b| {
        b.iter_batched(
            || {
                (
                    clips_edit(1, CLIPS, Play::Stretch),
                    Engine::new(config(512, 0)),
                )
            },
            |(edit, (engine, processor))| {
                (
                    EditSession::new(edit, engine).expect("the fixture compiles"),
                    processor,
                )
            },
            BatchSize::PerIteration,
        )
    });
    group.bench_function("handover", |b| {
        let (mut session, mut processor) =
            playing_edit(clips_edit(1, CLIPS, Play::Stretch), config(512, 0));
        let mut out = vec![0.0; 512 * 2];
        // Only the rebuild is timed; the block between adopts its graph, for
        // the next rebuild to hand its stretchers over.
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let started = Instant::now();
                session.rebuild().expect("the fixture compiles");
                total += started.elapsed();
                processor.process_interleaved(&mut out, 2);
            }
            total
        })
    });
    group.finish();
}

criterion_group!(benches, process_block, settings, rebuild);
criterion_main!(benches);
