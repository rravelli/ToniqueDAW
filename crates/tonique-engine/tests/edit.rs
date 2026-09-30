use std::sync::Arc;

use tonique_engine::automation::{BeatPoint, CurveShape};
use tonique_engine::edit::commands::*;
use tonique_engine::edit::{
    Bus, BusId, ChannelRef, Clip, ClipContent, Edit, EditError, EditSession, Note, Output, Plugin,
    PluginKind, Send, Track, TrackId,
};
use tonique_engine::engine::{AudioProcessor, Engine, EngineConfig, render_offline};
use tonique_engine::nodes::{Envelope, FilterMode};
use tonique_engine::sample::SampleBuffer;
use tonique_engine::time::BeatPos;

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
    for latency in [0, 100] {
        let mut t = Track::new(&mut edit, "t");
        let mut clip = Clip::audio(&mut edit, BeatPos(0.0), 4.0, src);
        clip.fade_in_s = 0.0;
        clip.fade_out_s = 0.0;
        t.clips.push(clip);
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
