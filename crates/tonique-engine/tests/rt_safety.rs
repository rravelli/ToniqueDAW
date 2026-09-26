//! Enforces "no allocation or deallocation on the RT path": this binary
//! installs the checking allocator, so any allocation inside
//! `AudioProcessor::process_interleaved` (including on helper workers) is
//! counted and fails the test.

use std::alloc::System;
use std::sync::Arc;

use tonique_engine::automation::{BeatPoint, CurveShape};
use tonique_engine::edit::commands::*;
use tonique_engine::edit::{Bus, ChannelRef, Clip, Edit, EditSession, Note, Plugin, PluginKind, Send, Track};
use tonique_engine::engine::{Engine, EngineConfig};
use tonique_engine::nodes::{Envelope, FilterMode};
use tonique_engine::rt::{self, CheckingAllocator};
use tonique_engine::sample::SampleBuffer;
use tonique_engine::time::BeatPos;

#[global_allocator]
static ALLOC: CheckingAllocator<System> = CheckingAllocator(System);

fn busy_edit() -> Edit {
    let mut edit = Edit::new(140.0);
    let sample = Arc::new(SampleBuffer::new(vec![(0..48000).map(|i| (i as f32 * 0.01).sin()).collect(); 2], 48000.0));
    let bus = Bus::new(&mut edit, "fx");
    for i in 0..12u8 {
        let mut t = Track::new(&mut edit, format!("t{i}"));
        if i % 2 == 0 {
            let synth = Plugin::new(&mut edit, PluginKind::Synth(Envelope::default()));
            t.channel.plugins.push(synth);
            let notes = (0..16).map(|n| Note { start: n as f64 * 0.25, length: 0.2, pitch: 40 + i + n as u8, velocity: 90 }).collect();
            let clip = Clip::midi(&mut edit, BeatPos(0.0), 4.0, notes);
            t.clips.push(clip);
        } else {
            let source = edit.add_source(sample.clone());
            let clip = Clip::audio(&mut edit, BeatPos(i as f64 * 0.1), 4.0, source);
            t.clips.push(clip);
            let lat = Plugin::new(&mut edit, PluginKind::Latency { samples: 64 * i as usize });
            t.channel.plugins.push(lat);
        }
        let filter = Plugin::new(&mut edit, PluginKind::Filter(FilterMode::HighPass));
        t.channel.plugins.push(filter);
        let send = Send::new(&mut edit, bus.id, 0.2);
        t.sends.push(send);
        t.channel.volume.automation = vec![
            BeatPoint { beat: BeatPos(0.0), value: 0.2, shape: CurveShape::Linear },
            BeatPoint { beat: BeatPos(2.0), value: 1.0, shape: CurveShape::Bezier { c1: 0.2, c2: 0.9 } },
            BeatPoint { beat: BeatPos(4.0), value: 0.5, shape: CurveShape::Step },
        ];
        edit.tracks.push(t);
    }
    edit.metronome.set(0.5, 0.0);
    let mut bus = bus;
    let echo = Plugin::new(&mut edit, PluginKind::Echo { time_s: 0.3 });
    bus.channel.plugins.push(echo);
    edit.buses.push(bus);
    edit
}

#[test]
fn rt_path_never_allocates() {
    let (engine, mut processor) = Engine::new(EngineConfig {
        max_block: 128,
        worker_threads: 3,
        parallel_threshold: 8,
        housekeeping_thread: false,
        ..Default::default()
    });
    let mut s = EditSession::new(busy_edit(), engine).unwrap();
    let tracks: Vec<_> = s.edit().tracks.iter().map(|t| (t.id, t.channel.volume.id, t.clips[0].id)).collect();
    s.set_loop(Some((BeatPos(0.5), BeatPos(3.5)))).unwrap();
    s.play().unwrap();

    // Odd device buffer size, so blocks get split and loops land mid-buffer.
    let mut device = vec![0.0f32; 2 * 441];
    for block in 0..2000 {
        processor.process_interleaved(&mut device, 2);
        assert!(device.iter().all(|x| x.is_finite()));

        // Control-thread activity interleaved with processing.
        let (tid, vol, cid) = tracks[block % tracks.len()];
        match block % 50 {
            7 => s.perform(MoveClip::new(tid, cid, BeatPos((block % 7) as f64 * 0.25))).unwrap(), // rebuild
            13 => s
                .perform(SetAutomation::new(vol, vec![BeatPoint { beat: BeatPos(0.0), value: 0.7, shape: CurveShape::Linear }]))
                .unwrap(), // curve swap via command ring
            21 => s.perform(SetMute::new(ChannelRef::Track(tid), block % 100 < 50)).unwrap(),
            29 => s.seek(BeatPos(1.0)).unwrap(),
            37 => {
                s.stop().unwrap();
                s.play().unwrap();
            }
            43 => {
                s.undo().unwrap();
            }
            47 => s.perform(ResizeClip::new(tid, cid, BeatPos(0.25), 3.0, 0.01)).unwrap(),
            // Preview: new streams arrive, old ones are retired.
            3 => s.engine_mut().preview_play(Box::new(Noise(44100.0)), 0).unwrap(),
            17 => s.engine_mut().preview_seek(1000).unwrap(),
            33 if block % 100 == 33 => s.engine_mut().preview_stop().unwrap(),
            49 => {
                // UI-side meter reads while the audio thread writes.
                s.edit().master.meter().take_levels();
                s.edit().master.meter().read_scope(0, &mut [0.0; 256]);
            }
            _ => {}
        }
        if block % 10 == 0 {
            s.engine().collect_garbage();
        }
    }
    assert!(s.engine().graphs_adopted() > 30);
    assert_eq!(rt::alloc_violations(), 0, "allocation on the RT path");

    // And the checker itself works.
    let v: Vec<u8> = rt::no_alloc(|| Vec::with_capacity(16));
    drop(v);
    assert_eq!(rt::alloc_violations(), 1);
}

/// Endless preview source at a different rate than the engine's.
struct Noise(f64);

impl tonique_engine::preview::PreviewSource for Noise {
    fn sample_rate(&self) -> f64 {
        self.0
    }
    fn seek(&mut self, _frame: usize) {}
    fn read(&mut self, left: &mut [f32], right: &mut [f32]) -> usize {
        for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            *l = (i as f32 * 0.37).sin() * 0.1;
            *r = -*l;
        }
        left.len()
    }
}
