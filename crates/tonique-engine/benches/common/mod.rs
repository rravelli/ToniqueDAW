//! Fixtures shared by the benchmarks and the `stress` example.

use std::sync::Arc;

use tonique_engine::automation::{BeatPoint, CurveShape};
use tonique_engine::edit::{Bus, Clip, Edit, EditSession, Note, Plugin, PluginKind, Send, Track};
use tonique_engine::engine::{AudioProcessor, Engine, EngineConfig};
use tonique_engine::nodes::{Envelope, FilterMode};
use tonique_engine::sample::SampleBuffer;
use tonique_engine::time::BeatPos;

pub const SAMPLE_RATE: f64 = 48000.0;
/// Length of the looped region, in beats.
pub const LOOP_BEATS: f64 = 4.0;

/// A session with `tracks` tracks, alternating synth and audio tracks, each
/// with a filter, a send to an echo bus and volume automation. Every block
/// does real work: notes and clips cover the whole loop.
pub fn busy_edit(tracks: usize) -> Edit {
    let mut edit = Edit::new(140.0);
    let sample = Arc::new(SampleBuffer::new(
        vec![(0..48000).map(|i| (i as f32 * 0.01).sin() * 0.5).collect(); 2],
        SAMPLE_RATE,
    ));
    let source = edit.add_source(sample);
    let mut bus = Bus::new(&mut edit, "fx");
    let echo = Plugin::new(&mut edit, PluginKind::Echo { time_s: 0.3 });
    bus.channel.plugins.push(echo);
    for i in 0..tracks {
        let mut t = Track::new(&mut edit, format!("t{i}"));
        if i % 2 == 0 {
            let synth = Plugin::new(&mut edit, PluginKind::Synth(Envelope::default()));
            t.channel.plugins.push(synth);
            let notes = (0..16)
                .map(|n| Note {
                    start: n as f64 * 0.25,
                    length: 0.2,
                    pitch: 40 + (i % 24) as u8 + n as u8,
                    velocity: 90,
                })
                .collect();
            t.clips
                .push(Clip::midi(&mut edit, BeatPos(0.0), LOOP_BEATS, notes));
        } else {
            t.clips
                .push(Clip::audio(&mut edit, BeatPos(0.0), LOOP_BEATS, source));
        }
        let filter = Plugin::new(&mut edit, PluginKind::Filter(FilterMode::HighPass));
        t.channel.plugins.push(filter);
        let send = Send::new(&mut edit, bus.id, 0.2);
        t.sends.push(send);
        t.channel.volume.automation = vec![
            BeatPoint {
                beat: BeatPos(0.0),
                value: 0.2,
                shape: CurveShape::Linear,
            },
            BeatPoint {
                beat: BeatPos(2.0),
                value: 1.0,
                shape: CurveShape::Bezier { c1: 0.2, c2: 0.9 },
            },
            BeatPoint {
                beat: BeatPos(4.0),
                value: 0.5,
                shape: CurveShape::Step,
            },
        ];
        edit.tracks.push(t);
    }
    edit.buses.push(bus);
    edit
}

pub fn config(max_block: usize, worker_threads: usize) -> EngineConfig {
    EngineConfig {
        sample_rate: SAMPLE_RATE,
        max_block,
        worker_threads,
        ..Default::default()
    }
}

/// A playing, looping session over [`busy_edit`], with its graph already
/// adopted by the processor. Keep the session alive while processing.
pub fn playing(tracks: usize, config: EngineConfig) -> (EditSession, AudioProcessor) {
    let (engine, mut processor) = Engine::new(config);
    let mut session = EditSession::new(busy_edit(tracks), engine).expect("the fixture compiles");
    session
        .set_loop(Some((BeatPos(0.0), BeatPos(LOOP_BEATS))))
        .expect("command ring has room");
    session.play().expect("command ring has room");
    // Adopt the graph and warm up voices, delay lines and caches.
    let mut out = vec![0.0; config.max_block * 2];
    for _ in 0..64 {
        processor.process_interleaved(&mut out, 2);
    }
    (session, processor)
}
