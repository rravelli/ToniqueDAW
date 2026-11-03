use std::sync::Arc;

use tonique_engine::automation::{BeatPoint, CurveShape};
use tonique_engine::edit::commands::*;
use tonique_engine::edit::{
    Bus, BusId, ChannelRef, Clip, ClipContent, ClipFades, ClipId, ClipLoop, Edit, EditError,
    EditSession, Fade, Note, Output, Plugin, PluginKind, SeamRoom, Send, StretchSettings, Track,
    TrackId, WarpMode, seams,
};
use tonique_engine::engine::{AudioProcessor, Engine, EngineConfig, render_offline};
use tonique_engine::nodes::{
    ClipWarp, Envelope, FadeCurve, FadeShape, FilterMode, UTILITY_SILENT_DB,
};
use tonique_engine::sample::SampleBuffer;
use tonique_engine::time::{BeatPos, TempoMap};

const SR: f64 = 48000.0;

fn engine(workers: usize) -> (Engine, AudioProcessor) {
    Engine::new(EngineConfig {
        sample_rate: SR,
        max_block: 256,
        output_channels: 2,
        worker_threads: workers,
        parallel_threshold: 4,
        housekeeping_thread: false,
    })
}

fn impulse() -> Arc<SampleBuffer> {
    let mut data = vec![0.0; 1000];
    data[0] = 1.0;
    Arc::new(SampleBuffer::new(vec![data], SR))
}

fn sine(len: usize) -> Arc<SampleBuffer> {
    Arc::new(SampleBuffer::new(
        vec![(0..len).map(|i| (i as f32 * 0.05).sin() * 0.5).collect()],
        SR,
    ))
}

fn synth_track(edit: &mut Edit, notes: Vec<Note>, length: f64) -> Track {
    let mut t = Track::new(edit, "synth");
    let synth = Plugin::new(edit, PluginKind::Synth(Envelope::default()));
    t.channel.plugins.push(synth);
    let clip = Clip::midi(edit, BeatPos(0.0), length, notes);
    t.clips.push(clip);
    t
}

fn note(start: f64, length: f64, pitch: u8) -> Note {
    Note {
        start,
        length,
        pitch,
        velocity: 100,
    }
}

fn peak(buf: &[f32]) -> f32 {
    buf.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

#[test]
fn renders_a_small_song() {
    let mut edit = Edit::new(120.0);
    let mut t = synth_track(
        &mut edit,
        vec![note(0.0, 1.0, 60), note(1.0, 1.0, 64), note(2.0, 1.0, 67)],
        4.0,
    );
    let echo = Plugin::new(&mut edit, PluginKind::Echo { time_s: 0.25 });
    t.channel.plugins.push(echo);
    let bus = Bus::new(&mut edit, "fx");
    let send = Send::new(&mut edit, bus.id, 0.5);
    t.sends.push(send);
    let mut bus = bus;
    let filter = Plugin::new(&mut edit, PluginKind::Filter(FilterMode::LowPass));
    bus.channel.plugins.push(filter);
    edit.tracks.push(t);
    edit.buses.push(bus);

    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    s.play().unwrap();
    let out = render_offline(&mut p, SR as usize * 2, 2);
    assert!(out.iter().all(|x| x.is_finite()));
    assert!(peak(&out) > 0.05, "song is silent");
    assert!((s.position().0 - 4.0).abs() < 1e-6);
}

#[test]
fn edits_preserve_node_state_bit_exactly() {
    // A note into an echo; after the note ends the echo tail keeps ringing.
    // Rebuilding the graph mid-tail must not disturb it at all.
    let build = || {
        let mut edit = Edit::new(120.0);
        let mut t = synth_track(&mut edit, vec![note(0.0, 0.25, 72)], 1.0);
        let echo = Plugin::new(&mut edit, PluginKind::Echo { time_s: 0.1 });
        echo.param("feedback").unwrap().set(0.9);
        t.channel.plugins.push(echo);
        edit.tracks.push(t);
        edit
    };
    let (e1, mut p1) = engine(0);
    let mut edited = EditSession::new(build(), e1).unwrap();
    let (e2, mut p2) = engine(0);
    let mut reference = EditSession::new(build(), e2).unwrap();
    edited.play().unwrap();
    reference.play().unwrap();

    let mut a = render_offline(&mut p1, 24000, 2);
    let mut b = render_offline(&mut p2, 24000, 2);

    // Unrelated structural edit while the tail rings.
    let empty = edited.create(|e| Track::new(e, "empty"));
    edited.perform(AddTrack::new(empty)).unwrap();
    a.extend(render_offline(&mut p1, 24000, 2));
    b.extend(render_offline(&mut p2, 24000, 2));

    assert_eq!(edited.engine().graphs_adopted(), 2);
    assert!(
        edited.engine().last_migrated() >= 3,
        "synth, echo and fader should carry state"
    );
    assert!(peak(&a[48000..]) > 0.01, "tail should still be ringing");
    assert_eq!(a, b, "rebuild changed the audio");
}

#[test]
fn undo_redo_and_transactions() {
    let mut edit = Edit::new(120.0);
    let t = synth_track(&mut edit, vec![note(0.0, 1.0, 60)], 4.0);
    let (tid, cid) = (t.id, t.clips[0].id);
    edit.tracks.push(t);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();

    let t2 = s.create(|e| Track::new(e, "second"));
    s.perform(AddTrack::new(t2)).unwrap();
    assert_eq!(s.edit().tracks.len(), 2);

    // A "drag": many moves, one undo step.
    s.begin_transaction("Drag clip");
    for i in 1..=8 {
        s.perform(MoveClip::new(tid, cid, BeatPos(i as f64 * 0.5)))
            .unwrap();
    }
    s.commit_transaction();
    assert_eq!(
        s.edit().track(tid).unwrap().clip(cid).unwrap().start,
        BeatPos(4.0)
    );

    assert!(s.undo().unwrap());
    assert_eq!(
        s.edit().track(tid).unwrap().clip(cid).unwrap().start,
        BeatPos(0.0)
    );
    assert!(s.undo().unwrap());
    assert_eq!(s.edit().tracks.len(), 1);
    assert!(!s.undo().unwrap());
    assert!(s.redo().unwrap());
    assert!(s.redo().unwrap());
    assert_eq!(s.edit().tracks.len(), 2);
    assert_eq!(
        s.edit().track(tid).unwrap().clip(cid).unwrap().start,
        BeatPos(4.0)
    );

    // The engine coalesced the burst: it's running the latest graph.
    render_offline(&mut p, 256, 2); // adopts the 8 queued graphs
    s.engine().collect_garbage(); // housekeeping hands over the pending one
    render_offline(&mut p, 256, 2);
    assert!(!s.engine().has_pending_graph());
    assert_eq!(s.engine().graphs_adopted(), 9); // 8 queued + the newest pending one
}

#[test]
fn mute_solo_and_params_do_not_rebuild() {
    let mut edit = Edit::new(120.0);
    let mut t = Track::new(&mut edit, "audio");
    let src = edit.add_source(sine(48000 * 8));
    let clip = Clip::audio(&mut edit, BeatPos(0.0), 16.0, src);
    t.clips.push(clip);
    let (tid, vol) = (t.id, t.channel.volume.id);
    edit.tracks.push(t);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    s.play().unwrap();
    assert!(peak(&render_offline(&mut p, 4800, 2)) > 0.1);

    s.perform(SetMute::new(ChannelRef::Track(tid), true))
        .unwrap();
    render_offline(&mut p, 4800, 2); // ramp out
    assert_eq!(peak(&render_offline(&mut p, 4800, 2)), 0.0);
    s.undo().unwrap();
    render_offline(&mut p, 4800, 2);
    assert!(peak(&render_offline(&mut p, 4800, 2)) > 0.1);

    s.perform(SetParam::new(vol, 0.0)).unwrap();
    render_offline(&mut p, 4800, 2);
    assert_eq!(peak(&render_offline(&mut p, 4800, 2)), 0.0);
    assert_eq!(
        s.engine().graphs_adopted(),
        1,
        "no rebuilds for mute/param changes"
    );
}

#[test]
fn automation_curves_swap_without_rebuild() {
    let mut edit = Edit::new(120.0);
    let mut t = Track::new(&mut edit, "audio");
    let src = edit.add_source(sine(48000 * 8));
    let clip = Clip::audio(&mut edit, BeatPos(0.0), 16.0, src);
    t.clips.push(clip);
    let vol = t.channel.volume.id;
    edit.tracks.push(t);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    let flat = |v: f32| {
        vec![BeatPoint {
            beat: BeatPos(0.0),
            value: v,
            shape: CurveShape::Linear,
        }]
    };

    s.perform(SetAutomation::new(vol, flat(0.0))).unwrap(); // lane appears: rebuild
    assert_eq!(s.last_compile_stats().unwrap().scheduled, 8); // incl. metronome + output sum
    s.play().unwrap();
    render_offline(&mut p, 4800, 2);
    assert_eq!(peak(&render_offline(&mut p, 4800, 2)), 0.0);

    s.perform(SetAutomation::new(vol, flat(1.0))).unwrap(); // lane edited: curve swap only
    render_offline(&mut p, 4800, 2);
    assert!(peak(&render_offline(&mut p, 4800, 2)) > 0.1);
    assert_eq!(s.engine().graphs_adopted(), 2);
    assert_eq!(
        s.engine().collect_garbage(),
        2,
        "old graph + old curve retired"
    );
}

#[test]
fn identical_clips_are_deduplicated_and_latency_is_compensated() {
    let mut edit = Edit::new(120.0);
    let src = edit.add_source(impulse());
    let mut clip = Clip::audio(&mut edit, BeatPos(0.0), 4.0, src);
    clip.fades.min_s = 0.0;
    for latency in [0, 100] {
        let mut t = Track::new(&mut edit, "t");
        // The same clip on both tracks, gain included: the same output.
        t.clips.push(clip.clone());
        if latency > 0 {
            let lat = Plugin::new(&mut edit, PluginKind::Latency { samples: latency });
            t.channel.plugins.push(lat);
        }
        edit.tracks.push(t);
    }
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    let stats = s.last_compile_stats().unwrap();
    assert_eq!(stats.deduplicated, 2, "clip reader and its sum are shared");
    assert_eq!(
        stats.delays_inserted, 2,
        "one on the latency-free track, one to keep the metronome in time"
    );
    assert_eq!(stats.output_latency, 100);
    s.play().unwrap();
    let out = render_offline(&mut p, 256, 2);
    let left: Vec<f32> = out.iter().step_by(2).copied().collect();
    assert_eq!(left[0], 0.0);
    assert!(
        (left[100] - 2.0).abs() < 1e-6,
        "both paths aligned at 100: {}",
        left[100]
    );
    assert_eq!(left.iter().filter(|x| **x != 0.0).count(), 1);
}

#[test]
fn parallel_rendering_matches_sequential() {
    let build = || {
        let mut edit = Edit::new(128.0);
        let bus = Bus::new(&mut edit, "verb");
        for i in 0..24 {
            let mut t = synth_track(
                &mut edit,
                vec![note(0.0, 0.5, 48 + i), note(1.0, 2.0, 50 + i)],
                4.0,
            );
            let f = Plugin::new(&mut edit, PluginKind::Filter(FilterMode::LowPass));
            t.channel.plugins.push(f);
            if i % 3 == 0 {
                let send = Send::new(&mut edit, bus.id, 0.3);
                t.sends.push(send);
            }
            if i % 5 == 0 {
                t.output = Output::Bus(bus.id);
            }
            edit.tracks.push(t);
        }
        let mut bus = bus;
        let echo = Plugin::new(&mut edit, PluginKind::Echo { time_s: 0.05 });
        bus.channel.plugins.push(echo);
        edit.buses.push(bus);
        edit
    };
    let (e1, mut p1) = engine(0);
    let (e2, mut p2) = engine(3);
    let mut s1 = EditSession::new(build(), e1).unwrap();
    let mut s2 = EditSession::new(build(), e2).unwrap();
    s1.play().unwrap();
    s2.play().unwrap();
    let a = render_offline(&mut p1, 48000, 2);
    let b = render_offline(&mut p2, 48000, 2);
    assert!(peak(&a) > 0.05);
    assert_eq!(a, b);
}

#[test]
fn moving_a_clip_mid_note_leaves_no_stuck_notes() {
    let mut edit = Edit::new(120.0);
    let t = synth_track(&mut edit, vec![note(0.0, 8.0, 60)], 8.0);
    let (tid, cid) = (t.id, t.clips[0].id);
    edit.tracks.push(t);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    s.play().unwrap();
    assert!(peak(&render_offline(&mut p, 12000, 2)) > 0.05);
    s.perform(MoveClip::new(tid, cid, BeatPos(32.0))).unwrap();
    render_offline(&mut p, 48000, 2); // release tail
    assert!(
        peak(&render_offline(&mut p, 4800, 2)) < 1e-4,
        "note is hanging"
    );
}

#[test]
fn stop_releases_notes_and_loop_retriggers() {
    let mut edit = Edit::new(120.0);
    let t = synth_track(&mut edit, vec![note(0.0, 4.0, 60)], 4.0);
    edit.tracks.push(t);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    s.set_loop(Some((BeatPos(0.0), BeatPos(1.0)))).unwrap();
    s.play().unwrap();
    // Loops every 0.5 s; keeps sounding.
    render_offline(&mut p, 48000 * 2, 2);
    assert!(s.position().0 < 1.0);
    assert!(peak(&render_offline(&mut p, 4800, 2)) > 0.05);
    s.stop().unwrap();
    render_offline(&mut p, 48000, 2);
    assert!(peak(&render_offline(&mut p, 4800, 2)) < 1e-4);
    // Content check: the MIDI clip's audio source matches the model.
    assert!(matches!(
        s.edit().tracks[0].clips[0].content,
        ClipContent::Midi { .. }
    ));
}

#[test]
fn clips_play_once_their_source_is_loaded() {
    let mut edit = Edit::new(120.0);
    let mut t = Track::new(&mut edit, "audio");
    let src = edit.new_source();
    let clip = Clip::audio(&mut edit, BeatPos(0.0), 16.0, src);
    t.clips.push(clip);
    edit.tracks.push(t);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    s.play().unwrap();
    assert_eq!(
        peak(&render_offline(&mut p, 4800, 2)),
        0.0,
        "silent while loading"
    );

    s.set_source(src, sine(48000 * 8)).unwrap();
    assert!(peak(&render_offline(&mut p, 4800, 2)) > 0.1);
    assert!(!s.undo_manager().can_undo(), "loading is not an edit");
}

#[test]
fn resize_and_move_clips_across_tracks_undo() {
    let mut edit = Edit::new(120.0);
    let src = edit.add_source(sine(48000 * 8));
    let mut a = Track::new(&mut edit, "a");
    let clip = Clip::audio(&mut edit, BeatPos(0.0), 8.0, src);
    let cid = clip.id;
    a.clips.push(clip);
    let b = Track::new(&mut edit, "b");
    let (ta, tb) = (a.id, b.id);
    edit.tracks.extend([a, b]);
    let (e, _p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();

    s.begin_transaction("Trim and move");
    s.perform(ResizeClip::new(ta, cid, BeatPos(1.0), 2.0, 0.5))
        .unwrap();
    s.perform(MoveClip::to_track(ta, cid, tb, BeatPos(4.0)))
        .unwrap();
    s.commit_transaction();
    assert!(s.edit().track(ta).unwrap().clips.is_empty());
    let c = s.edit().track(tb).unwrap().clip(cid).unwrap();
    assert_eq!((c.start, c.length), (BeatPos(4.0), 2.0));
    assert!(matches!(
        c.content,
        ClipContent::Audio {
            source_offset_s: 0.5,
            ..
        }
    ));

    assert!(s.undo().unwrap());
    assert!(s.edit().track(tb).unwrap().clips.is_empty());
    let c = s.edit().track(ta).unwrap().clip(cid).unwrap();
    assert_eq!((c.start, c.length), (BeatPos(0.0), 8.0));
    assert!(matches!(
        c.content,
        ClipContent::Audio {
            source_offset_s: 0.0,
            ..
        }
    ));
    assert!(
        s.perform(ResizeClip::new(ta, cid, BeatPos(0.0), 0.0, 0.0))
            .is_err()
    );
}

#[test]
fn clip_gain_is_heard_at_once_and_undone() {
    let mut edit = Edit::new(120.0);
    let src = edit.add_source(sine(48000 * 8));
    let mut track = Track::new(&mut edit, "t");
    let clip = Clip::audio(&mut edit, BeatPos(0.0), 8.0, src);
    let gain = clip.gain().expect("an audio clip has a gain").clone();
    track.clips.push(clip);
    edit.tracks.push(track);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    s.play().unwrap();
    let before = peak(&render_offline(&mut p, 4800, 2));

    // Set live, as while dragging: heard without rebuilding the graph.
    gain.set(0.5);
    render_offline(&mut p, 4800, 2); // through the ramp
    let half = peak(&render_offline(&mut p, 4800, 2));
    assert!((half - before / 2.).abs() < 0.01, "{before} then {half}");
    assert_eq!(s.engine().graphs_adopted(), 1, "no rebuild");

    // Recorded as a parameter change, and undone.
    gain.set(1.0);
    s.perform(SetParam::new(gain.id, 0.5)).unwrap();
    assert_eq!(gain.get(), 0.5);
    s.undo().unwrap();
    assert_eq!(gain.get(), 1.0);
}

/// A gain set while the clip is silent is its level from its first sample:
/// nothing ramped in from the old one.
#[test]
fn clip_gain_set_before_it_plays_is_not_ramped_in() {
    let mut edit = Edit::new(120.0);
    let src = edit.add_source(sine(48000 * 8));
    let mut track = Track::new(&mut edit, "t");
    // From 0.5 s: 24000 samples in.
    let clip = Clip::audio(&mut edit, BeatPos(1.0), 8.0, src);
    let gain = clip.gain().expect("an audio clip has a gain").clone();
    track.clips.push(clip);
    edit.tracks.push(track);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    s.play().unwrap();
    render_offline(&mut p, 4800, 2);
    gain.set(0.0);
    render_offline(&mut p, 4800 * 4, 2); // up to the clip
    let start = peak(&render_offline(&mut p, 4800, 2));
    assert!(start < 1e-6, "{start}");
}

#[test]
fn clip_pan_balances_live_and_is_undone() {
    let mut edit = Edit::new(120.0);
    let src = edit.add_source(sine(48000 * 8)); // mono: the same on both sides
    let mut track = Track::new(&mut edit, "t");
    let clip = Clip::audio(&mut edit, BeatPos(0.0), 8.0, src);
    let pan = clip.pan().expect("an audio clip has a pan").clone();
    track.clips.push(clip);
    edit.tracks.push(track);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    s.play().unwrap();
    let mut sides = || {
        render_offline(&mut p, 4800, 2); // through the ramp
        let out = render_offline(&mut p, 4800, 2);
        let side = |ch: usize| {
            out.iter()
                .skip(ch)
                .step_by(2)
                .fold(0f32, |m, x| m.max(x.abs()))
        };
        (side(0), side(1))
    };
    let (l, r) = sides();
    assert!((l - r).abs() < 1e-3 && l > 0.4, "centred: ({l}, {r})");

    // Halfway right: the left at half, the right as it was.
    pan.set(0.5);
    let (l2, r2) = sides();
    assert!(
        (l2 - l / 2.).abs() < 0.01 && (r2 - r).abs() < 0.01,
        "({l2}, {r2})"
    );
    // All the way left: the right is silent.
    pan.set(-1.0);
    let (l3, r3) = sides();
    assert!((l3 - l).abs() < 0.01 && r3 < 1e-3, "({l3}, {r3})");
    assert_eq!(s.engine().graphs_adopted(), 1, "no rebuild");

    pan.set(0.0);
    s.perform(SetParam::new(pan.id, 0.5)).unwrap();
    s.undo().unwrap();
    assert_eq!(pan.get(), 0.0);
}

/// A session playing, from its start, one track with `clip` on a source of
/// `samples`, and the clip's ID.
fn playing_clip(
    samples: Arc<SampleBuffer>,
    looped: Option<ClipLoop>,
    seconds: f64,
) -> (EditSession, AudioProcessor, TrackId, ClipId) {
    let mut edit = Edit::new(120.0);
    let src = edit.add_source(samples);
    let mut track = Track::new(&mut edit, "t");
    // 120 bpm: two beats a second.
    let mut clip = Clip::audio(&mut edit, BeatPos(0.0), seconds * 2., src);
    clip.fades.min_s = 0.0;
    if let ClipContent::Audio { looped: l, .. } = &mut clip.content {
        *l = looped;
    }
    let (tid, cid) = (track.id, clip.id);
    track.clips.push(clip);
    edit.tracks.push(track);
    let (e, p) = engine(0);
    let mut s = EditSession::new(edit, e).expect("session");
    s.play().expect("play");
    (s, p, tid, cid)
}

/// Silence with a 10 ms burst of 1 kHz at `at` seconds, `seconds` long.
fn burst(seconds: f64, at: f64) -> Arc<SampleBuffer> {
    let start = (at * SR) as usize;
    let wave = (0..(seconds * SR) as usize)
        .map(|i| {
            let into = i as f64 - start as f64;
            if (0.0..SR / 100.0).contains(&into) {
                (into / SR * 1000.0 * std::f64::consts::TAU).sin() as f32 * 0.5
            } else {
                0.0
            }
        })
        .collect();
    Arc::new(SampleBuffer::new(vec![wave], SR))
}

/// Where the loudest 5 ms of the left channel of `out` start, in seconds
/// from its start.
#[cfg(feature = "signalsmith")]
fn loudest(out: &[f32]) -> f64 {
    let window = (SR / 200.0) as usize;
    let left: Vec<f32> = out.iter().step_by(2).copied().collect();
    let energy = |w: usize| {
        left[w * window..(w + 1) * window]
            .iter()
            .map(|x| x * x)
            .sum::<f32>()
    };
    let best = (0..left.len() / window)
        .max_by(|a, b| energy(*a).total_cmp(&energy(*b)))
        .unwrap_or(0);
    (best * window) as f64 / SR
}

/// Resampled (no warp, transposed), a clip plays faster and higher, or
/// slower and lower: an octave up, its second of audio is over in half.
#[test]
fn transposed_unwarped_clips_play_resampled() {
    let (mut s, mut p, tid, cid) = playing_clip(sine(48000), None, 3.0);
    s.perform(SetClipWarp::new(tid, cid, WarpMode::Off, 120.0, 12.0))
        .unwrap();
    assert!(peak(&render_offline(&mut p, 19200, 2)) > 0.4, "0 to 0.4 s");
    render_offline(&mut p, 9600, 2);
    assert!(
        peak(&render_offline(&mut p, 9600, 2)) < 1e-6,
        "0.6 to 0.8 s"
    );
}

/// Stretched to half speed (the edit at 120 bpm, its source at 240), a
/// burst 1 s into the source is heard 2 s into the clip: from the start,
/// and after seeking past the clip's start, the stretcher fed from there.
#[cfg(feature = "signalsmith")]
#[test]
fn stretched_clips_play_on_time() {
    let (mut s, mut p, tid, cid) = playing_clip(burst(3.0, 1.0), None, 6.0);
    s.perform(SetClipWarp::new(tid, cid, WarpMode::Spectral, 240.0, 0.0))
        .unwrap();
    let heard = loudest(&render_offline(&mut p, (3.0 * SR) as usize, 2));
    assert!((heard - 2.0).abs() < 0.01, "heard at {heard} s");

    s.seek(BeatPos(3.0)).unwrap(); // 1.5 s
    let heard = 1.5 + loudest(&render_offline(&mut p, (1.0 * SR) as usize, 2));
    assert!(
        (heard - 2.0).abs() < 0.01,
        "after a seek, heard at {heard} s"
    );
}

/// Transposed while stretched, the pitch alone moves: an octave up, a
/// 1 kHz tone comes out at 2 kHz, as long as before, its formants kept or
/// not.
#[cfg(feature = "signalsmith")]
#[test]
fn stretched_clips_transpose_without_changing_speed() {
    for formants in [false, true] {
        let tone: Vec<f32> = (0..48000)
            .map(|i| (i as f64 / SR * 1000.0 * std::f64::consts::TAU).sin() as f32 * 0.5)
            .collect();
        let (mut s, mut p, tid, cid) =
            playing_clip(Arc::new(SampleBuffer::new(vec![tone], SR)), None, 3.0);
        s.perform(SetClipWarp::new(tid, cid, WarpMode::Spectral, 120.0, 12.0))
            .unwrap();
        let stretch = StretchSettings {
            formants,
            ..Default::default()
        };
        s.perform(SetClipStretch::new(tid, cid, stretch)).unwrap();
        render_offline(&mut p, 9600, 2);
        let out = render_offline(&mut p, 24000, 2);
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let crossings = left
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        // Half a second: about 1000 crossings at 2 kHz.
        assert!(
            (950..1050).contains(&crossings),
            "formants {formants}: {crossings}"
        );
        render_offline(&mut p, 9600, 2); // to 0.9 s
        assert!(
            peak(&render_offline(&mut p, 4800, 2)) > 0.2,
            "formants {formants}: still playing"
        );
    }
}

/// Stretched with any window, a clip plays on time; its window changed as
/// it plays, it gets a new stretcher, which starts over in the right
/// place.
#[cfg(feature = "signalsmith")]
#[test]
fn stretched_clips_play_on_time_whatever_their_window() {
    for window in tonique_engine::edit::StretchWindow::ALL {
        let (mut s, mut p, tid, cid) = playing_clip(burst(3.0, 1.0), None, 6.0);
        s.perform(SetClipWarp::new(tid, cid, WarpMode::Spectral, 240.0, 0.0))
            .unwrap();
        let stretch = StretchSettings {
            window,
            ..Default::default()
        };
        s.perform(SetClipStretch::new(tid, cid, stretch)).unwrap();
        let heard = loudest(&render_offline(&mut p, (3.0 * SR) as usize, 2));
        assert!((heard - 2.0).abs() < 0.01, "{window:?}: heard at {heard} s");

        s.seek(BeatPos(0.0)).unwrap();
        let mut out = render_offline(&mut p, (1.0 * SR) as usize, 2);
        s.undo().unwrap(); // back to the normal window
        out.extend(render_offline(&mut p, (2.0 * SR) as usize, 2));
        let heard = loudest(&out);
        assert!(
            (heard - 2.0).abs() < 0.01,
            "{window:?} then normal: heard at {heard} s"
        );
    }
}

/// Rebuilt as it plays, a stretched clip keeps its stretcher (handed over
/// to the new graph) and its timing; and keeps playing through graphs
/// replaced before they ever played.
#[cfg(feature = "signalsmith")]
#[test]
fn stretched_clips_play_on_through_rebuilds() {
    let (mut s, mut p, tid, cid) = playing_clip(burst(3.0, 1.0), None, 6.0);
    s.perform(SetClipWarp::new(tid, cid, WarpMode::Spectral, 240.0, 0.0))
        .unwrap();
    let mut out = Vec::new();
    for _ in 0..30 {
        out.extend(render_offline(&mut p, 4800, 2));
        s.engine().collect_garbage();
        s.rebuild().unwrap();
    }
    let heard = loudest(&out);
    assert!((heard - 2.0).abs() < 0.01, "heard at {heard} s");

    // Stretched again in a graph replaced before it plays (the engine
    // takes 8 at most): its stretcher is new in the one replacing it.
    s.perform(SetClipWarp::new(tid, cid, WarpMode::Off, 240.0, 0.0))
        .unwrap();
    s.seek(BeatPos(3.0)).unwrap(); // 1.5 s
    render_offline(&mut p, 512, 2);
    for _ in 0..8 {
        s.rebuild().unwrap();
    }
    s.perform(SetClipWarp::new(tid, cid, WarpMode::Spectral, 240.0, 0.0))
        .unwrap();
    s.rebuild().unwrap();
    // The graphs queued play, then the one left waiting.
    render_offline(&mut p, 256, 2);
    s.engine().collect_garbage();
    s.seek(BeatPos(3.0)).unwrap();
    let heard = 1.5 + loudest(&render_offline(&mut p, (1.0 * SR) as usize, 2));
    assert!(
        (heard - 2.0).abs() < 0.01,
        "after graphs never played, heard at {heard} s"
    );
}

/// The tempo changing while playing, the transport stays on its beat and
/// goes on at the new tempo: it doesn't jump to where its samples fall.
#[test]
fn the_transport_keeps_its_beat_when_the_tempo_changes() {
    let (mut s, mut p, _, _) = playing_clip(sine(48000 * 4), None, 4.0);
    render_offline(&mut p, 48000, 2); // 1 s at 120 bpm: beat 2
    assert!((s.position().0 - 2.0).abs() < 1e-3, "{:?}", s.position());
    s.perform(SetTempoMap::new(TempoMap::new(60.0))).unwrap();
    render_offline(&mut p, 48000, 2); // 1 s at 60 bpm: a beat further
    assert!((s.position().0 - 3.0).abs() < 0.01, "{:?}", s.position());
    s.undo().unwrap();
    render_offline(&mut p, 48000, 2); // back at 120: two more
    assert!((s.position().0 - 5.0).abs() < 0.01, "{:?}", s.position());
}

/// A seek and a loop asked in beats land on their beats whatever tempo
/// reaches the engine with them.
#[test]
fn seeks_and_loops_land_on_their_beats_across_tempo_changes() {
    let (mut s, mut p, _, _) = playing_clip(sine(48000 * 8), None, 8.0);
    render_offline(&mut p, 256, 2);
    s.set_loop(Some((BeatPos(0.0), BeatPos(4.0)))).unwrap();
    s.seek(BeatPos(3.0)).unwrap();
    s.perform(SetTempoMap::new(TempoMap::new(60.0))).unwrap();
    // 1.5 s at 60 bpm: to beat 4, back to 0, half a beat on.
    render_offline(&mut p, 72000, 2);
    assert!((s.position().0 - 0.5).abs() < 0.01, "{:?}", s.position());
}

/// Stretched (its source at 240 bpm), a clip goes on through a tempo
/// change, its stretcher playing on at the new speed, and stays on its
/// beats: a burst on beat 4 of its source is heard on beat 4, at the new
/// tempo.
#[cfg(feature = "signalsmith")]
#[test]
fn stretched_clips_stay_on_their_beats_when_the_tempo_changes() {
    let (mut s, mut p, tid, cid) = playing_clip(burst(3.0, 1.0), None, 6.0);
    s.perform(SetClipWarp::new(tid, cid, WarpMode::Spectral, 240.0, 0.0))
        .unwrap();
    let mut out = render_offline(&mut p, 24000, 2); // beat 1 at 120 bpm
    s.perform(SetTempoMap::new(TempoMap::new(200.0))).unwrap();
    out.extend(render_offline(&mut p, (2.0 * SR) as usize, 2));
    // Beat 1 at 0.5 s, then three more at 200 bpm.
    let heard = loudest(&out);
    assert!((heard - 1.4).abs() < 0.01, "heard at {heard} s");
}

/// A warp is numbers: a transposition or a source tempo that isn't one is
/// refused, and one that got into a clip anyway plays as if it weren't set.
#[test]
fn warps_are_numbers() {
    let (mut s, _p, tid, cid) = playing_clip(sine(48000), None, 1.0);
    for (bpm, transpose) in [
        (120.0, f32::NAN),
        (120.0, f32::INFINITY),
        (f64::INFINITY, 0.0),
    ] {
        let set = SetClipWarp::new(tid, cid, WarpMode::Spectral, bpm, transpose);
        assert!(s.perform(set).is_err(), "{bpm} bpm, {transpose}");
    }
    assert_eq!(
        WarpMode::Off.play(120.0, f32::NAN, StretchSettings::default(), 120.0),
        ClipWarp::None
    );
    assert_eq!(
        WarpMode::Spectral.play(f64::NAN, f32::NAN, StretchSettings::default(), 120.0),
        ClipWarp::None
    );
}

/// Stretched, a clip follows the tempo where it is: at 90 bpm after a
/// change from 120, a 90 bpm source plays as it is.
#[test]
fn stretched_clips_follow_the_tempo_where_they_are() {
    let mut edit = Edit::new(120.0);
    edit.tempo.set_tempo(BeatPos(32.0), 90.0);
    let src = edit.add_source(sine(48000));
    let mut clip = Clip::audio(&mut edit, BeatPos(44.0), 8.0, src);
    if let ClipContent::Audio {
        warp, source_bpm, ..
    } = &mut clip.content
    {
        *warp = WarpMode::Spectral;
        *source_bpm = 90.0;
    }
    assert_eq!(clip.warp(&edit.tempo), Some(ClipWarp::None));
    // Across the change, at its average: 4 beats at each.
    clip.start = BeatPos(28.0);
    let Some(ClipWarp::Stretch { speed, .. }) = clip.warp(&edit.tempo) else {
        panic!("not stretched");
    };
    assert!(
        (speed - 8.0 / (2.0 + 8.0 / 3.0) * 60.0 / 90.0).abs() < 1e-9,
        "{speed}"
    );
}

/// The tempo raised with the transport a sample before the loop's end,
/// both rounding to the same sample: the loop still goes round.
#[test]
fn loops_go_round_when_the_tempo_moves_the_transport_onto_their_end() {
    let (mut s, mut p, _, _) = playing_clip(sine(48000 * 8), None, 8.0);
    s.set_loop(Some((BeatPos(0.0), BeatPos(4.0)))).unwrap();
    // Beat 4 at 120 bpm is sample 96000: seek to 95998, play one.
    s.seek(BeatPos(95998.0 / 24000.0)).unwrap();
    render_offline(&mut p, 1, 2);
    // At 143 bpm, 95999 and 96000 both fall on 80559.
    s.perform(SetTempoMap::new(TempoMap::new(143.0))).unwrap();
    render_offline(&mut p, 48000, 2);
    assert!(s.position().0 < 4.0, "{:?}", s.position());
}

/// Held back, changes reach the engine at once: one graph for many.
#[test]
fn held_effects_rebuild_once() {
    let (mut s, mut p, tid, cid) = playing_clip(burst(3.0, 1.0), None, 6.0);
    render_offline(&mut p, 512, 2);
    let adopted = s.engine().graphs_adopted();
    s.hold_effects();
    for bpm in [100.0, 110.0, 120.0] {
        s.perform(SetClipWarp::new(tid, cid, WarpMode::Off, bpm, 0.0))
            .unwrap();
    }
    s.release_effects().unwrap();
    render_offline(&mut p, 512, 2);
    assert_eq!(s.engine().graphs_adopted(), adopted + 1);
}

#[test]
fn looped_clips_go_on_from_the_loop_start() {
    // Each sample its own value, to tell where reading is.
    let len = 48000;
    let ramp: Vec<f32> = (0..len).map(|i| i as f32 / len as f32).collect();
    let source = Arc::new(SampleBuffer::new(vec![ramp.clone()], SR));
    // Loop 0.25 s to 0.5 s of a 1 s file, over 1 s of clip.
    let looped = ClipLoop {
        start_s: 0.25,
        end_s: 0.5,
    };
    let (_s, mut p, _, _) = playing_clip(source, Some(looped), 1.0);
    let out = render_offline(&mut p, 48000, 2);
    let left: Vec<f32> = out.iter().step_by(2).copied().collect();
    // The intro plays once, then the loop over and over.
    for t in [100, 12_000, 23_000, 24_000 + 100, 30_000, 47_000] {
        let expected = if t < 24_000 {
            ramp[t]
        } else {
            ramp[12_000 + (t - 12_000) % 12_000]
        };
        assert!(
            (left[t] - expected).abs() < 1e-5,
            "at {t}: {} vs {expected}",
            left[t]
        );
    }
}

#[test]
fn loop_seams_are_crossfaded() {
    // A sine whose loop doesn't hold a whole number of periods: cut
    // straight, the seam would jump.
    let source = sine(48000 * 2);
    let looped = ClipLoop {
        start_s: 0.5,
        end_s: 0.5 + 0.1003,
    };
    let (_s, mut p, _, _) = playing_clip(source, Some(looped), 1.0);
    let out = render_offline(&mut p, 48000, 2);
    let left: Vec<f32> = out.iter().step_by(2).copied().collect();
    // A 0.5 sine moving 0.05 rad a sample changes by less than 0.025 per
    // sample; a seam would jump much more.
    let jump = left
        .windows(2)
        .skip(100)
        .take(47_000)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0f32, f32::max);
    assert!(jump < 0.03, "a sample jumps by {jump}");
}

#[test]
fn clip_loops_are_set_and_undone() {
    let (mut s, mut p, tid, cid) = playing_clip(sine(48000 * 2), None, 4.0);
    let looped = ClipLoop {
        start_s: 0.0,
        end_s: 0.5,
    };
    s.perform(SetClipLoop::new(tid, cid, Some(looped))).unwrap();
    let content = |s: &EditSession| match &s.edit().track(tid).unwrap().clip(cid).unwrap().content {
        ClipContent::Audio { looped, .. } => *looped,
        ClipContent::Midi { .. } => unreachable!(),
    };
    assert_eq!(content(&s), Some(looped));
    // Past the 2 s file: still playing, looped.
    render_offline(&mut p, 48000 * 3, 2);
    assert!(peak(&render_offline(&mut p, 4800, 2)) > 0.4);
    s.undo().unwrap();
    assert_eq!(content(&s), None);

    let backwards = ClipLoop {
        start_s: 1.0,
        end_s: 0.5,
    };
    assert!(
        s.perform(SetClipLoop::new(tid, cid, Some(backwards)))
            .is_err()
    );
}

#[test]
fn a_clips_bar_1_is_set_and_undone_without_a_rebuild() {
    let (mut s, _p, tid, cid) = playing_clip(sine(48000 * 2), None, 4.0);
    let origin = |s: &EditSession| match &s.edit().track(tid).unwrap().clip(cid).unwrap().content {
        ClipContent::Audio { origin_s, .. } => *origin_s,
        ClipContent::Midi { .. } => unreachable!(),
    };
    assert_eq!(origin(&s), 0.0, "the file's start");
    let graphs = s.engine().graphs_adopted();
    s.perform(SetClipOrigin::new(tid, cid, 0.25)).unwrap();
    assert_eq!(origin(&s), 0.25);
    assert_eq!(s.engine().graphs_adopted(), graphs, "not heard");
    s.undo().unwrap();
    assert_eq!(origin(&s), 0.0);
}

#[test]
fn tracks_can_be_reordered_renamed_and_duplicated() {
    let mut edit = Edit::new(120.0);
    let t = synth_track(&mut edit, vec![note(0.0, 1.0, 60)], 4.0);
    let (tid, vol) = (t.id, t.channel.volume.id);
    edit.tracks.push(t);
    let (e, _p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();

    let original = s.edit().track(tid).unwrap().clone();
    let copy = s.create(|e| original.duplicate(e));
    let (cid, cvol) = (copy.id, copy.channel.volume.id);
    assert_ne!(cvol, vol);
    assert_ne!(copy.clips[0].id, original.clips[0].id);
    assert_ne!(copy.channel.plugins[0].id, original.channel.plugins[0].id);
    s.perform(AddTrack::new(copy)).unwrap();
    s.perform(SetParam::new(cvol, 0.5)).unwrap();
    assert_eq!(
        s.edit().param(vol).unwrap().get(),
        1.0,
        "copy has its own params"
    );

    s.perform(MoveTrack::new(cid, 0)).unwrap();
    s.perform(RenameTrack::new(cid, "copy")).unwrap();
    assert_eq!(s.edit().tracks[0].id, cid);
    assert_eq!(s.edit().tracks[0].name, "copy");
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(s.edit().tracks[1].id, cid);
    assert_eq!(s.edit().tracks[1].name, "synth");
}

#[test]
fn meters_report_post_fader_levels() {
    let mut edit = Edit::new(120.0);
    let mut t = Track::new(&mut edit, "audio");
    let src = edit.add_source(sine(48000 * 8));
    let clip = Clip::audio(&mut edit, BeatPos(0.0), 16.0, src);
    t.clips.push(clip);
    let tid = t.id;
    edit.tracks.push(t);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    s.play().unwrap();
    render_offline(&mut p, 4800, 2);
    let [l, _] = s
        .edit()
        .track(tid)
        .unwrap()
        .channel
        .meter()
        .take_levels()
        .unwrap();
    assert!(
        (l.peak - 0.5).abs() < 0.01,
        "sine peaks at 0.5, got {}",
        l.peak
    );
    assert!((l.rms - 0.5 / 2f32.sqrt()).abs() < 0.01);
    assert!(s.edit().master.meter().take_levels().unwrap()[1].peak > 0.4);

    let vol = s.edit().track(tid).unwrap().channel.volume.id;
    s.perform(SetParam::new(vol, 0.0)).unwrap();
    render_offline(&mut p, 4800, 2); // ramp down
    s.edit().track(tid).unwrap().channel.meter().take_levels();
    render_offline(&mut p, 4800, 2);
    assert_eq!(
        s.edit()
            .track(tid)
            .unwrap()
            .channel
            .meter()
            .take_levels()
            .unwrap()[0]
            .peak,
        0.0
    );

    let mut scope = [0.0; 64];
    s.edit().master.meter().read_scope(0, &mut scope);
    assert!(scope.iter().all(|x| *x == 0.0));
}

#[test]
fn metronome_clicks_on_beats_when_enabled() {
    let edit = Edit::new(120.0);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    s.play().unwrap();
    assert_eq!(
        peak(&render_offline(&mut p, 24000, 2)),
        0.0,
        "off by default"
    );

    s.edit().metronome.set(1.0, 0.0);
    s.seek(BeatPos(0.0)).unwrap();
    let out = render_offline(&mut p, 48000, 2); // two beats at 120 bpm
    let left: Vec<f32> = out.iter().step_by(2).copied().collect();
    assert!(peak(&left[..4800]) > 0.5, "click on beat 1");
    assert_eq!(peak(&left[4800..24000]), 0.0, "silence between beats");
    assert!(peak(&left[24000..28800]) > 0.5, "click on beat 2");

    s.stop().unwrap();
    assert_eq!(
        peak(&render_offline(&mut p, 4800, 2)),
        0.0,
        "silent when stopped"
    );
}

#[test]
fn topology_shows_the_compiled_graph_with_labels() {
    let mut edit = Edit::new(120.0);
    let src = edit.add_source(sine(48000 * 8));
    let mut drums = Track::new(&mut edit, "drums");
    let clip = Clip::audio(&mut edit, BeatPos(0.0), 16.0, src);
    drums.clips.push(clip);
    let filter = Plugin::new(&mut edit, PluginKind::Filter(FilterMode::LowPass));
    drums.channel.plugins.push(filter);
    drums.channel.volume.automation = vec![BeatPoint {
        beat: BeatPos(0.0),
        value: 0.5,
        shape: CurveShape::Linear,
    }];
    let mut slow = Track::new(&mut edit, "slow");
    let lat = Plugin::new(&mut edit, PluginKind::Latency { samples: 64 });
    slow.channel.plugins.push(lat);
    edit.tracks.extend([drums, slow]);
    let (e, _p) = engine(0);
    let s = EditSession::new(edit, e).unwrap();

    let t = s.engine().graph_topology().expect("a graph was published");
    assert_eq!(t.stats, s.last_compile_stats().unwrap());
    assert_eq!(t.nodes.len(), t.stats.scheduled);
    let find = |label: &str| {
        t.nodes
            .iter()
            .position(|n| n.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("no node {label}"))
    };
    let (clip, sum, lp, fader) = (
        find("drums · clip"),
        find("drums · clips Σ"),
        find("drums · low-pass"),
        find("drums · fader"),
    );
    assert_eq!(t.nodes[clip].name, "AudioClipNode");
    assert_eq!(t.nodes[sum].inputs, [clip]);
    assert_eq!(t.nodes[lp].inputs, [sum]);
    assert_eq!(t.nodes[fader].inputs, [lp]);
    assert_eq!(
        t.nodes[fader].after,
        [find("drums · volume automation")],
        "automation runs first"
    );
    assert_eq!(t.nodes[find("slow · latency")].latency_samples, 64);
    // The drums branch is delayed to line up with the slow track.
    let comp = find("latency comp. +64");
    assert_eq!(t.nodes[comp].inputs, [fader]);
    assert_eq!(t.nodes[t.output].label.as_deref(), Some("output"));
    // Nodes know their track; the compensation delay belongs to the delayed track.
    let drums_id = s.edit().tracks[0].id.0;
    for n in [
        clip,
        sum,
        lp,
        fader,
        comp,
        find("drums · volume automation"),
    ] {
        assert_eq!(t.nodes[n].owner, Some(drums_id), "{:?}", t.nodes[n].label);
    }
    assert_eq!(t.nodes[t.output].owner, None);
    assert_eq!(t.nodes[t.output].total_latency, 64);
    // Schedule order: inputs always come first.
    assert!(
        t.nodes
            .iter()
            .enumerate()
            .all(|(i, n)| n.inputs.iter().chain(&n.after).all(|&p| p < i))
    );
}

#[test]
fn node_meters_measure_only_when_enabled() {
    let mut edit = Edit::new(120.0);
    let src = edit.add_source(sine(48000 * 8));
    let mut t = Track::new(&mut edit, "audio");
    let clip = Clip::audio(&mut edit, BeatPos(0.0), 16.0, src);
    t.clips.push(clip);
    edit.tracks.push(t);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    s.play().unwrap();
    let topology = s.engine().graph_topology().unwrap();
    let fader = topology
        .nodes
        .iter()
        .position(|n| n.label.as_deref() == Some("audio · fader"))
        .unwrap();

    render_offline(&mut p, 4800, 2);
    assert!(
        topology
            .meters()
            .take()
            .iter()
            .all(|r| *r == Default::default()),
        "off by default"
    );

    s.engine_mut().set_graph_metering(true);
    render_offline(&mut p, 4800, 2);
    let readings = topology.meters().take();
    assert!(
        (readings[fader].peak - 0.5).abs() < 0.01,
        "fader peak {}",
        readings[fader].peak
    );
    assert!(readings.iter().any(|r| !r.busy.is_zero()));
    assert_eq!(
        topology.meters().take()[fader].peak,
        0.0,
        "reset after reading"
    );

    // New graphs inherit the setting.
    s.perform(MoveClip::new(
        s.edit().tracks[0].id,
        s.edit().tracks[0].clips[0].id,
        BeatPos(0.0),
    ))
    .unwrap();
    render_offline(&mut p, 4800, 2);
    let rebuilt = s.engine().graph_topology().unwrap();
    assert!(!Arc::ptr_eq(&rebuilt, &topology));
    assert!(rebuilt.meters().take().iter().any(|r| r.peak > 0.4));
}

#[test]
fn the_edit_moves_to_a_new_engine_with_its_history() {
    let mut edit = Edit::new(120.0);
    let t = synth_track(&mut edit, vec![note(0.0, 4.0, 60)], 4.0);
    edit.tracks.push(t);
    let (e, _old_p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    let t2 = s.create(|e| Track::new(e, "second"));
    s.perform(AddTrack::new(t2)).unwrap();

    let (e, mut p) = Engine::new(EngineConfig {
        sample_rate: 44100.0,
        max_block: 128,
        housekeeping_thread: false,
        ..Default::default()
    });
    let old = s.replace_engine(e).unwrap();
    assert_eq!(old.config().sample_rate, SR);
    assert_eq!(s.engine().config().sample_rate, 44100.0);
    s.play().unwrap();
    assert!(
        peak(&render_offline(&mut p, 4410, 2)) > 0.05,
        "plays on the new engine"
    );
    assert!(s.undo().unwrap(), "history survives");
    assert_eq!(s.edit().tracks.len(), 1);
}

#[test]
fn reset_replaces_the_edit_and_forgets_history() {
    let (engine, _processor) = engine(0);
    let mut session = EditSession::new(Edit::new(120.0), engine).unwrap();
    let track = session.create(|e| Track::new(e, "a"));
    session.perform(AddTrack::at(track, 0)).unwrap();
    assert!(session.undo_manager().can_undo());

    session.reset(Edit::new(90.0)).unwrap();
    assert!(session.edit().tracks.is_empty());
    assert_eq!(session.edit().tempo.bpm_at(BeatPos(0.0)), 90.0);
    assert!(!session.undo_manager().can_undo());
    assert!(!session.undo().unwrap());
}

/// A synth track routed through `inner` bus, itself routed into `outer`.
fn nested_groups() -> (Edit, TrackId, TrackId, BusId, BusId) {
    let mut edit = Edit::new(120.0);
    let mut grouped = synth_track(&mut edit, vec![note(0.0, 1.0, 60)], 1.0);
    let outside = synth_track(&mut edit, vec![note(0.0, 1.0, 67)], 1.0);
    let outer = Bus::new(&mut edit, "outer");
    let mut inner = Bus::new(&mut edit, "inner");
    inner.output = Output::Bus(outer.id);
    grouped.output = Output::Bus(inner.id);
    let ids = (grouped.id, outside.id, inner.id, outer.id);
    edit.tracks.extend([grouped, outside]);
    edit.buses.extend([outer, inner]);
    (edit, ids.0, ids.1, ids.2, ids.3)
}

#[test]
fn audio_flows_through_nested_buses() {
    let (mut edit, _, outside, _, _) = nested_groups();
    edit.tracks.retain(|t| t.id != outside);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    s.play().unwrap();
    assert!(
        peak(&render_offline(&mut p, 12000, 2)) > 0.05,
        "nothing reached the output"
    );
}

#[test]
fn soloing_a_bus_solos_everything_inside() {
    let (edit, grouped, outside, inner, outer) = nested_groups();
    let (e, _p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    let audible = |s: &EditSession, id| s.edit().track(id).unwrap().channel.audible();

    s.perform(SetSolo::bus(outer, true)).unwrap();
    assert!(
        audible(&s, grouped),
        "inside the soloed bus, two levels down"
    );
    assert!(!audible(&s, outside));
    s.undo().unwrap();
    assert!(audible(&s, outside));

    // Soloing a track inside a group keeps its path audible.
    s.perform(SetSolo::new(grouped, true)).unwrap();
    assert!(audible(&s, grouped));
    assert!(!audible(&s, outside));
    assert!(s.perform(SetSolo::bus(inner, true)).is_ok());
}

/// A channel muted before the engine starts is silent from the first
/// sample: its mute gain doesn't fade out from full.
#[test]
fn muted_channels_are_silent_from_the_start() {
    let mut edit = Edit::new(120.0);
    let src = edit.add_source(sine(48000));
    let mut t = Track::new(&mut edit, "muted");
    let clip = Clip::audio(&mut edit, BeatPos(0.0), 1.0, src);
    t.clips.push(clip);
    t.channel.muted = true;
    edit.tracks.push(t);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    s.play().unwrap();
    assert_eq!(peak(&render_offline(&mut p, 2048, 2)), 0.);
}

/// Soloing a track in a detached copy (an export's), or moving its
/// parameters (as its automation does), leaves what the original plays
/// alone.
#[test]
fn soloing_a_detached_copy_leaves_the_original_alone() {
    let (edit, grouped, outside, _, _) = nested_groups();
    let mut copy = edit.detached();
    copy.track_mut(grouped).unwrap().soloed = true;
    copy.refresh_mute_gains();
    assert!(!copy.track(outside).unwrap().channel.audible());
    assert!(edit.track(outside).unwrap().channel.audible());

    let volume = edit.track(grouped).unwrap().channel.volume.clone();
    copy.param(volume.id).unwrap().value.set(0.25, 0.0);
    assert_eq!(copy.param(volume.id).unwrap().get(), 0.25);
    assert_eq!(volume.get(), volume.default);

    // A plain clone shares the gains (why `detached` exists).
    let mut clone = edit.clone();
    clone.track_mut(grouped).unwrap().soloed = true;
    clone.refresh_mute_gains();
    assert!(!edit.track(outside).unwrap().channel.audible());
}

#[test]
fn buses_cannot_feed_into_themselves() {
    let (edit, _, _, inner, outer) = nested_groups();
    let (e, _p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    // `inner` already feeds `outer`: the other way round would loop.
    assert!(matches!(
        s.perform(SetBusOutput::new(outer, Output::Bus(inner))),
        Err(EditError::RoutingCycle(_))
    ));
    assert!(matches!(
        s.perform(SetBusOutput::new(outer, Output::Bus(outer))),
        Err(EditError::RoutingCycle(_))
    ));
    s.perform(SetBusOutput::new(inner, Output::Master)).unwrap();
    s.perform(SetBusOutput::new(outer, Output::Bus(inner)))
        .unwrap();
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(s.edit().bus(inner).unwrap().output, Output::Bus(outer));
}

#[test]
fn buses_are_removed_once_unused_and_renamed() {
    let (edit, grouped, _, inner, outer) = nested_groups();
    let (e, _p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    assert!(
        s.perform(RemoveBus::new(inner)).is_err(),
        "a track still routes into it"
    );

    s.perform(SetOutput::new(grouped, Output::Master)).unwrap();
    s.perform(RemoveBus::new(inner)).unwrap();
    assert!(s.edit().bus(inner).is_err());
    s.perform(RenameBus::new(outer, "Drums")).unwrap();
    assert_eq!(s.edit().bus(outer).unwrap().name, "Drums");

    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(s.edit().bus(outer).unwrap().name, "outer");
    assert_eq!(s.edit().bus(inner).unwrap().output, Output::Bus(outer));
}

#[test]
fn plugin_taps_record_input_and_output_only_while_watched() {
    let mut edit = Edit::new(120.0);
    let src = edit.add_source(sine(48000 * 4));
    let mut track = Track::new(&mut edit, "t");
    let clip = Clip::audio(&mut edit, BeatPos(0.0), 8.0, src);
    track.clips.push(clip);
    // A low-pass far below the sine (~380 Hz): the output is much quieter.
    let filter = Plugin::new(&mut edit, PluginKind::Filter(FilterMode::LowPass));
    filter.param("cutoff").unwrap().set(40.0);
    let tap = filter.tap.clone();
    track.channel.plugins.push(filter);
    edit.tracks.push(track);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    s.play().unwrap();

    render_offline(&mut p, 4800, 2);
    assert_eq!(
        tap.input.take_levels(),
        None,
        "not watched: nothing recorded"
    );

    tap.watch_input_for(9600);
    tap.watch_output_for(9600);
    render_offline(&mut p, 4800, 2);
    let input = tap.input.take_levels().unwrap()[0];
    let output = tap.output.take_levels().unwrap()[0];
    assert!(
        (input.peak - 0.5).abs() < 0.01,
        "before the filter: {input:?}"
    );
    assert!(output.peak < 0.1, "after it: {output:?}");
    let mut scope = vec![0.0; 1024];
    tap.input.read_scope(0, &mut scope);
    assert!(peak(&scope) > 0.4);

    // Watched for 9600 frames: it stops on its own.
    render_offline(&mut p, 9600, 2);
    tap.input.take_levels();
    render_offline(&mut p, 4800, 2);
    assert_eq!(tap.input.take_levels(), None);

    // Each side only when it's watched.
    tap.output.take_levels();
    tap.watch_output_for(4800);
    render_offline(&mut p, 4800, 2);
    assert_eq!(tap.input.take_levels(), None);
    assert!(tap.output.take_levels().is_some());
}

#[test]
fn analyzers_pass_audio_through_untouched_and_tap_it() {
    let render = |analyzer: bool| {
        let mut edit = Edit::new(120.0);
        let src = edit.add_source(sine(48000 * 4));
        let mut track = Track::new(&mut edit, "t");
        let clip = Clip::audio(&mut edit, BeatPos(0.0), 8.0, src);
        track.clips.push(clip);
        let plugin = Plugin::new(&mut edit, PluginKind::Analyzer);
        assert!(plugin.params.is_empty());
        let tap = plugin.tap.clone();
        tap.watch_output_for(48000);
        if analyzer {
            track.channel.plugins.push(plugin);
        }
        edit.tracks.push(track);
        let (e, mut p) = engine(0);
        let mut s = EditSession::new(edit, e).unwrap();
        s.play().unwrap();
        (render_offline(&mut p, 4800, 2), tap)
    };
    let (with, tap) = render(true);
    let (without, _) = render(false);
    assert_eq!(with, without);
    assert!((tap.output.take_levels().unwrap()[0].peak - 0.5).abs() < 0.01);
}

#[test]
fn plugins_can_be_renamed_with_undo() {
    let mut edit = Edit::new(120.0);
    let mut track = Track::new(&mut edit, "t");
    let plugin = Plugin::new(&mut edit, PluginKind::Analyzer);
    let id = plugin.id;
    assert_eq!(plugin.name, None);
    track.channel.plugins.push(plugin);
    let tid = track.id;
    edit.tracks.push(track);
    let (e, _p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    let name = |s: &EditSession| s.edit().track(tid).unwrap().channel.plugins[0].name.clone();

    s.perform(RenamePlugin::new(
        ChannelRef::Track(tid),
        id,
        Some("Tops".into()),
    ))
    .unwrap();
    assert_eq!(name(&s).as_deref(), Some("Tops"));
    s.undo().unwrap();
    assert_eq!(name(&s), None);
    s.redo().unwrap();
    assert_eq!(name(&s).as_deref(), Some("Tops"));
}

#[test]
fn utility_shapes_gain_balance_width_and_phase() {
    // Left: a sine; right: silent. Mid is half the sine, side the other half.
    let len = 48000 * 8;
    let left: Vec<f32> = (0..len).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    let src_buf = Arc::new(SampleBuffer::new(vec![left, vec![0.0; len]], SR));
    let mut edit = Edit::new(120.0);
    let src = edit.add_source(src_buf);
    let mut track = Track::new(&mut edit, "t");
    let clip = Clip::audio(&mut edit, BeatPos(0.0), 16.0, src);
    track.clips.push(clip);
    let utility = Plugin::new(&mut edit, PluginKind::Utility);
    let params = utility.params.clone();
    track.channel.plugins.push(utility);
    edit.tracks.push(track);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    s.play().unwrap();
    let set = |name: &str, value: f32| {
        params.iter().find(|p| p.name == name).unwrap().set(value);
    };
    // Peaks of each side once ramps have settled; the sign of the left's
    // first sample past them tells its phase against the source's.
    let mut render = || {
        let out = render_offline(&mut p, 4800, 2);
        let tail = &out[2 * 2400..];
        let side = |ch: usize| {
            tail.iter()
                .skip(ch)
                .step_by(2)
                .fold(0f32, |m, s| m.max(s.abs()))
        };
        (side(0), side(1))
    };
    let close = |(l, r): (f32, f32), (el, er): (f32, f32)| {
        assert!(
            (l - el).abs() < 0.01 && (r - er).abs() < 0.01,
            "({l}, {r}), expected ({el}, {er})"
        );
    };

    close(render(), (0.5, 0.0));
    set("gain", -6.0206);
    close(render(), (0.25, 0.0));
    set("gain", 0.0);
    set("width", 0.0);
    close(render(), (0.25, 0.25));
    set("width", 2.0);
    // Side doubled: L = mid + 2·side, R = mid − 2·side.
    close(render(), (0.75, 0.25));
    set("mono", 1.0);
    close(render(), (0.25, 0.25));
    set("balance", -1.0);
    close(render(), (0.25, 0.0));
    set("gain", UTILITY_SILENT_DB);
    close(render(), (0.0, 0.0));
}

#[test]
fn utility_inverts_each_side() {
    let mut edit = Edit::new(120.0);
    let src = edit.add_source(sine(48000 * 4));
    let mut track = Track::new(&mut edit, "t");
    let clip = Clip::audio(&mut edit, BeatPos(0.0), 8.0, src);
    track.clips.push(clip);
    let utility = Plugin::new(&mut edit, PluginKind::Utility);
    utility.param("invert_left").unwrap().set(1.0);
    track.channel.plugins.push(utility);
    edit.tracks.push(track);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).unwrap();
    s.play().unwrap();
    let out = render_offline(&mut p, 4800, 2);
    // A mono source: the left now mirrors the right.
    for frame in out.chunks(2).skip(100) {
        assert!((frame[0] + frame[1]).abs() < 1e-5, "{frame:?}");
    }
    assert!(peak(&out) > 0.4);
}

/// A source at unity all along, `seconds` long.
fn level(seconds: f64) -> Arc<SampleBuffer> {
    Arc::new(SampleBuffer::new(
        vec![vec![1.0; (seconds * SR) as usize]],
        SR,
    ))
}

/// The left channel of a session of `clips` at 120 bpm (two beats a second)
/// over a level, each `(start, length)` in beats with its fades, for
/// `seconds`. The clips are hard left and hard right in turn.
fn faded(clips: &[(f64, f64, ClipFades)], seconds: f64) -> (Vec<f32>, Vec<f32>) {
    let mut edit = Edit::new(120.0);
    let src = edit.add_source(level(60.0));
    let mut track = Track::new(&mut edit, "t");
    for (i, (start, length, fades)) in clips.iter().enumerate() {
        let mut clip = Clip::audio(&mut edit, BeatPos(*start), *length, src);
        clip.fades = *fades;
        if clips.len() > 1 {
            clip.pan()
                .expect("audio")
                .value
                .set(if i == 0 { -1.0 } else { 1.0 }, 0.0);
        }
        track.clips.push(clip);
    }
    edit.tracks.push(track);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).expect("session");
    s.play().expect("play");
    channels(&render_offline(&mut p, (seconds * SR) as usize, 2))
}

fn channels(out: &[f32]) -> (Vec<f32>, Vec<f32>) {
    (
        out.iter().step_by(2).copied().collect(),
        out.iter().skip(1).step_by(2).copied().collect(),
    )
}

fn at(out: &[f32], seconds: f64) -> f32 {
    out[(seconds * SR) as usize]
}

fn fade(length_s: f64, curve: FadeCurve) -> Fade {
    Fade { length_s, curve }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 2e-3
}

#[test]
fn fades_have_their_shapes_gain_at_their_start_middle_and_end() {
    for shape in FadeShape::ALL {
        let curve = FadeCurve::of(shape).with_curvature(0.25);
        let fades = ClipFades {
            fade_in: fade(0.5, curve),
            fade_out: fade(0.25, curve),
            ..Default::default()
        };
        // Two seconds.
        let (out, _) = faded(&[(0.0, 4.0, fades)], 2.5);
        assert_eq!(out[0], 0.0, "{shape:?} starts silent");
        assert!(near(at(&out, 0.25), curve.gain(0.5)), "{shape:?} fading in");
        assert!(near(at(&out, 0.5), 1.0), "{shape:?} faded in");
        assert!(near(at(&out, 1.0), 1.0), "{shape:?} between its fades");
        assert!(near(at(&out, 1.75), 1.0), "{shape:?} about to fade out");
        assert!(
            near(at(&out, 1.875), curve.gain(0.5)),
            "{shape:?} fading out"
        );
        assert!(at(&out, 1.9999) < 0.01, "{shape:?} ends silent");
        assert_eq!(at(&out, 2.1), 0.0, "{shape:?} over");
    }
}

/// With no fade, a short ramp all the same, so a clip doesn't click.
#[test]
fn clips_without_fades_still_have_their_short_ramp() {
    let (out, _) = faded(&[(0.0, 2.0, ClipFades::default())], 1.5);
    assert_eq!(out[0], 0.0);
    assert!(near(at(&out, 0.0025), 0.5));
    assert!(near(at(&out, 0.005), 1.0));
    assert!(near(at(&out, 1.0 - 0.0025), 0.5));
}

#[test]
fn setting_fades_is_an_undo_step() {
    let (mut s, mut p, track, clip) = {
        let mut edit = Edit::new(120.0);
        let src = edit.add_source(level(4.0));
        let mut t = Track::new(&mut edit, "t");
        let c = Clip::audio(&mut edit, BeatPos(0.0), 4.0, src);
        let ids = (t.id, c.id);
        t.clips.push(c);
        edit.tracks.push(t);
        let (e, p) = engine(0);
        (EditSession::new(edit, e).expect("session"), p, ids.0, ids.1)
    };
    let fades = ClipFades {
        fade_in: fade(1.0, FadeCurve::LINEAR),
        ..Default::default()
    };
    s.perform(SetClipFades::new(track, clip, fades))
        .expect("set");
    s.play().expect("play");
    let (out, _) = channels(&render_offline(&mut p, 24000, 2));
    assert!(near(at(&out, 0.25), 0.25), "heard at once");
    let clip_fades = |s: &EditSession| s.edit().track(track).unwrap().clip(clip).unwrap().fades;
    assert_eq!(clip_fades(&s), fades);
    s.undo().expect("undo");
    assert_eq!(clip_fades(&s), ClipFades::default());
    s.redo().expect("redo");
    assert_eq!(clip_fades(&s), fades);
    // Heard on audio clips only.
    let mut edit = Edit::new(120.0);
    let t = synth_track(&mut edit, vec![], 4.0);
    let mut command = SetClipFades::new(t.id, t.clips[0].id, fades);
    edit.tracks.push(t);
    let midi = command.apply(&mut edit).unwrap_err();
    assert_eq!(midi, EditError::Invalid("not an audio clip"));
}

/// Fades that don't fit are heard scaled down together, on a clip resized
/// (or at a tempo) that leaves them too little room; given the room again,
/// they're back.
#[test]
fn fades_stay_inside_their_clip_when_it_is_resized() {
    let fades = ClipFades {
        fade_in: fade(0.75, FadeCurve::LINEAR),
        fade_out: fade(0.25, FadeCurve::LINEAR),
        ..Default::default()
    };
    // Half a second: 3/8 and 1/8 of a second.
    let mut edit = Edit::new(120.0);
    let src = edit.add_source(level(4.0));
    let mut t = Track::new(&mut edit, "t");
    let mut c = Clip::audio(&mut edit, BeatPos(0.0), 4.0, src);
    c.fades = fades;
    let (track, clip) = (t.id, c.id);
    t.clips.push(c);
    edit.tracks.push(t);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).expect("session");
    s.perform(ResizeClip::new(track, clip, BeatPos(0.0), 1.0, 0.0))
        .expect("resize");
    s.play().expect("play");
    let (out, _) = channels(&render_offline(&mut p, 24000, 2));
    assert!(near(at(&out, 0.1875), 0.5), "half way in");
    assert!(near(at(&out, 0.375), 1.0), "faded in");
    assert!(near(at(&out, 0.4375), 0.5), "half way out");

    let heard = fades.heard(0.5, None, None);
    assert_eq!((heard.0.length_s, heard.1.length_s), (0.375, 0.125));
    let heard = fades.heard(2.0, None, None);
    assert_eq!((heard.0.length_s, heard.1.length_s), (0.75, 0.25));
}

/// Fades belong to the clip, not to the passes of its loop.
#[test]
fn looped_clips_fade_once_over_their_whole_length() {
    let mut edit = Edit::new(120.0);
    let src = edit.add_source(level(0.25));
    let mut t = Track::new(&mut edit, "t");
    let mut c = Clip::audio(&mut edit, BeatPos(0.0), 4.0, src);
    c.fades.fade_in = fade(0.5, FadeCurve::LINEAR);
    c.fades.fade_out = fade(0.5, FadeCurve::LINEAR);
    if let ClipContent::Audio { looped, .. } = &mut c.content {
        *looped = Some(ClipLoop {
            start_s: 0.0,
            end_s: 0.25,
        });
    }
    t.clips.push(c);
    edit.tracks.push(t);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).expect("session");
    s.play().expect("play");
    let (out, _) = channels(&render_offline(&mut p, 96000, 2));
    assert!(near(at(&out, 0.3), 0.6), "through the second pass");
    assert!(near(at(&out, 1.0), 1.0), "between the fades");
    assert!(near(at(&out, 1.75), 0.5), "fading out in its last passes");
}

/// A render apart from the edit (an export) hears the fades and crossfades.
#[test]
fn fades_are_heard_in_a_detached_edit() {
    let mut edit = Edit::new(120.0);
    let src = edit.add_source(level(4.0));
    let mut t = Track::new(&mut edit, "t");
    let mut c = Clip::audio(&mut edit, BeatPos(0.0), 2.0, src);
    c.fades.fade_in = fade(0.5, FadeCurve::EQUAL_POWER);
    let mut late = Clip::audio(&mut edit, BeatPos(2.0), 2.0, src);
    late.fades.fade_out = fade(0.5, FadeCurve::LINEAR);
    t.clips.extend([c, late]);
    edit.tracks.push(t);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit.detached(), e).expect("session");
    s.play().expect("play");
    let (out, _) = channels(&render_offline(&mut p, 96000, 2));
    assert!(near(at(&out, 0.25), FadeCurve::EQUAL_POWER.gain(0.5)));
    assert!(near(at(&out, 1.75), 0.5), "the later clip fading out");
}

/// A session of a level's two clips touching at a second, hard left and
/// hard right, the later one crossing as `crossfade` says. Each plays `spare`
/// seconds of file trimmed off at the seam (`spare.0` off the later clip's
/// start, `spare.1` off the earlier clip's end).
fn seam(crossfade: Fade, spare: (f64, f64), seconds: f64) -> (Vec<f32>, Vec<f32>) {
    let mut edit = Edit::new(120.0);
    // Its first second, and what's after, for the earlier clip to read on.
    let earlier_src = edit.add_source(level(1.0 + spare.1));
    let later_src = edit.add_source(level(2.0 + spare.0));
    let mut t = Track::new(&mut edit, "t");
    let earlier = Clip::audio(&mut edit, BeatPos(0.0), 2.0, earlier_src);
    let mut later = Clip::audio(&mut edit, BeatPos(2.0), 2.0, later_src);
    later.fades.crossfade = crossfade;
    if let ClipContent::Audio {
        source_offset_s, ..
    } = &mut later.content
    {
        *source_offset_s = spare.0;
    }
    earlier.pan().expect("audio").value.set(-1.0, 0.0);
    later.pan().expect("audio").value.set(1.0, 0.0);
    t.clips.extend([earlier, later]);
    edit.tracks.push(t);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).expect("session");
    s.play().expect("play");
    channels(&render_offline(&mut p, (seconds * SR) as usize, 2))
}

/// Across a seam, each clip plays on past it as the other fades in, neither
/// moving: equal power by default, keeping the power at any curvature.
#[test]
fn a_crossfade_plays_both_clips_on_past_their_seam() {
    for curvature in [0.0, 0.6, -0.6] {
        let curve = FadeCurve::EQUAL_POWER.with_curvature(curvature);
        let (l, r) = seam(fade(0.5, curve), (1.0, 1.0), 1.5);
        assert!(near(at(&l, 0.5), 1.0), "the first alone");
        assert_eq!(at(&r, 0.5), 0.0, "the second not yet");
        for t in [0.76, 0.85, 1.0, 1.1, 1.24] {
            let power = at(&l, t).powi(2) + at(&r, t).powi(2);
            assert!(near(power, 1.0), "{curvature} at {t}: {power}");
        }
        let half = FadeCurve::EQUAL_POWER.with_curvature(curvature).gain(0.5);
        assert!(near(at(&r, 1.0), half), "half way, at the seam");
        assert!(near(at(&r, 1.3), 1.0), "the second alone");
        assert_eq!(at(&l, 1.3), 0.0);
    }
}

/// The later clip starting early reads what's before it in its file, so
/// after the crossfade it plays where it always did.
#[test]
fn a_crossfade_reads_what_is_trimmed_off() {
    let mut edit = Edit::new(120.0);
    let ramp: Vec<f32> = (0..96000).map(|i| i as f32 / 96000.0).collect();
    let src = edit.add_source(Arc::new(SampleBuffer::new(vec![ramp], SR)));
    let mut t = Track::new(&mut edit, "t");
    let earlier = Clip::audio(&mut edit, BeatPos(0.0), 1.0, src);
    let mut later = Clip::audio(&mut edit, BeatPos(1.0), 2.0, src);
    later.fades.crossfade = fade(0.2, FadeCurve::LINEAR);
    if let ClipContent::Audio {
        source_offset_s, ..
    } = &mut later.content
    {
        *source_offset_s = 1.0;
    }
    earlier.pan().expect("audio").value.set(-1.0, 0.0);
    later.pan().expect("audio").value.set(1.0, 0.0);
    t.clips.extend([earlier, later]);
    edit.tracks.push(t);
    let (e, mut p) = engine(0);
    let mut s = EditSession::new(edit, e).expect("session");
    s.play().expect("play");
    let (l, r) = channels(&render_offline(&mut p, 48000, 2));
    // At the seam, half a second in: half of each.
    assert!(
        near(at(&l, 0.5), 0.25 * 0.5),
        "the earlier one's file reads on"
    );
    assert!(near(at(&r, 0.5), 0.5 * 0.5), "the later one's from 1 s");
    assert!(near(at(&r, 0.75), 0.625), "past it: as it always did");
}

/// With nothing trimmed off one side, it all goes on the other side; with
/// nothing on either, no crossfade: each ends with its short ramp.
#[test]
fn a_crossfade_only_goes_where_there_is_audio() {
    // Nothing before the later clip's start: all after the seam.
    let (l, r) = seam(fade(0.5, FadeCurve::LINEAR), (0.0, 1.0), 1.75);
    assert_eq!(at(&r, 0.9), 0.0, "not starting early");
    assert!(near(at(&l, 0.9), 1.0));
    assert!(near(at(&r, 1.25), 0.5), "half way");
    assert!(near(at(&l, 1.25), 0.5));
    assert!(near(at(&r, 1.5), 1.0), "done");

    // Nothing past the earlier clip's end: all before the seam.
    let (l, r) = seam(fade(0.5, FadeCurve::LINEAR), (1.0, 0.0), 1.25);
    assert!(near(at(&l, 0.75), 0.5), "half way");
    assert!(near(at(&r, 0.75), 0.5));
    assert_eq!(at(&l, 1.1), 0.0, "over at its end");

    // Neither: as if there were none.
    let (l, r) = seam(fade(0.5, FadeCurve::LINEAR), (0.0, 0.0), 1.25);
    assert!(near(at(&l, 0.9), 1.0));
    assert_eq!(at(&r, 0.9), 0.0);
    assert!(near(at(&r, 1.1), 1.0));
}

/// A crossfade replaces the clip's own fade at that end, which comes back
/// once they no longer touch.
#[test]
fn crossfades_replace_the_fades_at_their_ends_only_at_a_seam() {
    let long = ClipFades {
        fade_out: fade(0.75, FadeCurve::LINEAR),
        ..Default::default()
    };
    let crossing = ClipFades {
        crossfade: fade(0.5, FadeCurve::EQUAL_POWER),
        ..Default::default()
    };
    // Both from the start of a long file: it all goes after the seam.
    let clips = [(0.0, 2.0, long), (2.0, 2.0, crossing)];
    let (l, _) = faded(&clips, 1.4);
    assert!(near(at(&l, 0.9), 1.0), "not fading before the seam");
    assert!(near(
        at(&l, 1.25),
        FadeCurve::EQUAL_POWER.crossing().gain(0.5)
    ));
    // Apart: its own fade, and no crossfade.
    let clips = [(0.0, 2.0, long), (2.5, 2.0, crossing)];
    let (l, r) = faded(&clips, 1.3);
    assert!(near(at(&l, 0.625), 0.5), "its fade");
    assert_eq!(at(&r, 1.2), 0.0, "not starting early");
}

#[test]
fn seams_are_where_a_clip_starts_as_another_ends() {
    let found = seams(&[(4.0, 8.0), (0.0, 4.0), (9.0, 10.0), (8.0000001, 9.0)]);
    assert_eq!(found, [Some(1), None, Some(3), Some(0)]);
    assert_eq!(
        seams(&[(0.0, 4.0), (3.0, 5.0)]),
        [None, None],
        "overlapping"
    );
}

#[test]
fn seam_room_shares_a_crossfade_between_its_sides() {
    let room = SeamRoom::new(10.0, 10.0, 10.0, 10.0);
    assert_eq!(room.window(2.0), (1.0, 1.0), "half each");
    assert_eq!(room.length_reaching(1.0), 2.0);
    // Little before the seam: the rest goes after it.
    let room = SeamRoom::new(0.5, 10.0, 10.0, 10.0);
    assert_eq!(room.window(2.0), (0.5, 1.5));
    assert_eq!(room.length_reaching(1.5), 2.0, "reaching where it goes");
    // Nothing either side: no crossfade.
    let room = SeamRoom::new(0.0, 0.0, 10.0, 10.0);
    assert_eq!(room.window(2.0), (0.0, 0.0));
    assert_eq!(room.length_reaching(2.0), 0.0);
    // No further than the clip it crosses, however much file there is.
    let room = SeamRoom::new(10.0, 10.0, 1.0, 2.0);
    assert_eq!(room.window(8.0), (1.0, 2.0));
    // Not more than asked of a looped clip's endless audio.
    let room = SeamRoom::new(1.0, f64::INFINITY, 10.0, 10.0);
    assert_eq!(room.window(1.0), (0.5, 0.5));
}
