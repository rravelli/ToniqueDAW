//! Monitoring through the whole engine, with a fake input device.

use tonique_engine::edit::commands::{AddTrack, RemoveTrack, SetParam};
use tonique_engine::edit::{Edit, EditSession, Plugin, PluginKind, Track, TrackId};
use tonique_engine::engine::{AudioProcessor, Engine, EngineConfig, EngineError, render_offline};
use tonique_engine::input::{InputFeed, InputRoute, InputWriter};

const BLOCK: usize = 64;
const CHANNELS: usize = 3;

fn session(worker_threads: usize) -> (EditSession, AudioProcessor) {
    let (engine, processor) = Engine::new(EngineConfig {
        max_block: BLOCK,
        worker_threads,
        parallel_threshold: 1,
        housekeeping_thread: false,
        ..Default::default()
    });
    (
        EditSession::new(Edit::new(120.0), engine).expect("new session"),
        processor,
    )
}

fn add_track(s: &mut EditSession) -> TrackId {
    let track = s.create(|e| Track::new(e, "t"));
    let id = track.id;
    s.perform(AddTrack::new(track)).expect("add track");
    id
}

/// An input whose channels each have their own constant level.
fn connect(s: &mut EditSession) -> InputWriter {
    let (feed, writer) = InputFeed::new(CHANNELS, BLOCK, 1);
    s.engine_mut().set_input(Some(feed)).expect("set input");
    writer
}

/// `frames` frames with channel `c` at `levels[c]`.
fn feed_levels(writer: &mut InputWriter, levels: [f32; CHANNELS], frames: usize) {
    let samples = levels.into_iter().cycle().take(frames * CHANNELS);
    let samples: Vec<f32> = samples.collect();
    writer.push(samples.into_iter());
}

/// Render `frames` frames fed with `levels`, a block at a time so the ring
/// never overflows; returns the last frame, the faders having settled.
fn settled(
    p: &mut AudioProcessor,
    writer: &mut InputWriter,
    levels: [f32; CHANNELS],
    frames: usize,
) -> [f32; 2] {
    let mut last = Vec::new();
    for _ in 0..frames / BLOCK {
        feed_levels(writer, levels, BLOCK);
        last = render_offline(p, BLOCK, 2);
    }
    [last[last.len() - 2], last[last.len() - 1]]
}

fn assert_close(actual: [f32; 2], expected: [f32; 2]) {
    for (a, e) in actual.iter().zip(&expected) {
        assert!((a - e).abs() < 1e-3, "{actual:?} instead of {expected:?}");
    }
}

#[test]
fn a_track_not_monitoring_is_silent() {
    let (mut s, mut p) = session(0);
    add_track(&mut s);
    let mut w = connect(&mut s);
    assert_close(settled(&mut p, &mut w, [0.5; 3], 4096), [0.0, 0.0]);
    assert_eq!(s.engine().position(), 0, "the transport stays stopped");
}

#[test]
fn a_mono_route_reaches_both_sides_with_the_transport_stopped() {
    let (mut s, mut p) = session(0);
    let t = add_track(&mut s);
    let mut w = connect(&mut s);
    s.set_track_monitor(t, Some(InputRoute::Mono(1))).unwrap();
    let out = settled(&mut p, &mut w, [0.1, 0.5, 0.9], 4096);
    assert_close(out, [0.5, 0.5]);
    assert!(!s.engine().is_playing());
}

#[test]
fn a_stereo_route_takes_a_pair() {
    let (mut s, mut p) = session(0);
    let t = add_track(&mut s);
    let mut w = connect(&mut s);
    s.set_track_monitor(t, Some(InputRoute::Stereo(1))).unwrap();
    let out = settled(&mut p, &mut w, [0.1, 0.5, 0.9], 4096);
    assert_close(out, [0.5, 0.9]);
}

#[test]
fn channels_the_device_lacks_are_silent() {
    let (mut s, mut p) = session(0);
    let t = add_track(&mut s);
    let mut w = connect(&mut s);
    // The right side of this pair is past the last channel.
    s.set_track_monitor(t, Some(InputRoute::Stereo(2))).unwrap();
    let out = settled(&mut p, &mut w, [0.1, 0.5, 0.9], 4096);
    assert_close(out, [0.9, 0.0]);

    s.set_track_monitor(t, Some(InputRoute::Mono(CHANNELS)))
        .unwrap();
    let out = settled(&mut p, &mut w, [0.1, 0.5, 0.9], 4096);
    assert_close(out, [0.0, 0.0]);
}

#[test]
fn monitoring_without_an_input_is_silent() {
    let (mut s, mut p) = session(0);
    let t = add_track(&mut s);
    s.set_track_monitor(t, Some(InputRoute::Mono(0))).unwrap();
    assert!(render_offline(&mut p, 256, 2).iter().all(|x| *x == 0.0));
}

#[test]
fn monitoring_goes_through_the_track_effects_and_fader() {
    let (mut s, mut p) = session(0);
    let t = add_track(&mut s);
    let mut w = connect(&mut s);
    // A plugin delaying by 100 frames: the input comes out that much later.
    let delay = 100;
    let latency = s.create(|e| Plugin::new(e, PluginKind::Latency { samples: delay }));
    let utility = s.create(|e| Plugin::new(e, PluginKind::Utility));
    let invert = utility.param("invert_left").unwrap().id;
    s.create(|e| {
        let plugins = &mut e.track_mut(t).unwrap().channel.plugins;
        plugins.push(latency);
        plugins.push(utility);
    });
    let volume = s.edit().track(t).unwrap().channel.volume.id;
    s.set_track_monitor(t, Some(InputRoute::Mono(0))).unwrap();

    // Settle on silence so only the ramp's delay shows.
    settled(&mut p, &mut w, [0.0; 3], 4096);
    let mut out = Vec::new();
    for block in 0..3 {
        let samples: Vec<f32> = (0..BLOCK)
            .flat_map(|i| [(block * BLOCK + i + 1) as f32 * 1e-3, 0.0, 0.0])
            .collect();
        w.push(samples.into_iter());
        out.extend(render_offline(&mut p, BLOCK, 2));
    }
    assert!(out[..2 * delay].iter().all(|x| x.abs() < 1e-6));
    let first = out[2 * delay + 1];
    assert!(
        (first - 1e-3).abs() < 1e-5,
        "right side, frame {delay}: {first}"
    );

    s.perform(SetParam::new(invert, 1.0)).unwrap();
    s.perform(SetParam::new(volume, 0.5)).unwrap();
    let out = settled(&mut p, &mut w, [0.5, 0.0, 0.0], 24000);
    assert_close(out, [-0.25, 0.25]);
}

#[test]
fn tracks_on_the_same_channels_add_up_also_on_workers() {
    let (mut s, mut p) = session(3);
    let mut w = connect(&mut s);
    for _ in 0..8 {
        let t = add_track(&mut s);
        s.set_track_monitor(t, Some(InputRoute::Mono(0))).unwrap();
        // Make room for the next graphs (there's no housekeeping thread).
        settled(&mut p, &mut w, [0.0; 3], BLOCK);
        s.engine().collect_garbage();
    }
    let out = settled(&mut p, &mut w, [0.1, 0.0, 0.0], 4096);
    assert_close(out, [0.8, 0.8]);
}

#[test]
fn turning_monitoring_off_silences_the_track_and_leaves_no_undo_step() {
    let (mut s, mut p) = session(0);
    let t = add_track(&mut s);
    let mut w = connect(&mut s);
    settled(&mut p, &mut w, [0.5; 3], BLOCK);
    let adopted = s.engine().graphs_adopted();
    s.set_track_monitor(t, Some(InputRoute::Mono(0))).unwrap();
    settled(&mut p, &mut w, [0.5; 3], 4096);
    assert_eq!(s.engine().graphs_adopted(), adopted + 1);

    // Undoing the track's creation isn't what it takes back.
    assert_eq!(s.undo_manager().undo_label(), Some("Add track"));

    // Setting what's already set costs no rebuild.
    s.set_track_monitor(t, Some(InputRoute::Mono(0))).unwrap();
    settled(&mut p, &mut w, [0.5; 3], 256);
    assert_eq!(s.engine().graphs_adopted(), adopted + 1);

    s.set_track_monitor(t, None).unwrap();
    assert_close(settled(&mut p, &mut w, [0.5; 3], 4096), [0.0, 0.0]);
}

#[test]
fn a_removed_track_comes_back_monitoring() {
    let (mut s, mut p) = session(0);
    let t = add_track(&mut s);
    let mut w = connect(&mut s);
    s.set_track_monitor(t, Some(InputRoute::Mono(0))).unwrap();
    s.perform(RemoveTrack::new(t)).unwrap();
    assert_close(settled(&mut p, &mut w, [0.5; 3], 4096), [0.0, 0.0]);
    s.undo().unwrap();
    let out = settled(&mut p, &mut w, [0.5; 3], 4096);
    assert_close(out, [0.5, 0.5]);
}

#[test]
fn a_copied_track_does_not_monitor() {
    let (mut s, _p) = session(0);
    let t = add_track(&mut s);
    s.set_track_monitor(t, Some(InputRoute::Mono(0))).unwrap();
    let original = s.edit().track(t).unwrap();
    let mut edit = Edit::new(120.0);
    assert_eq!(original.duplicate(&mut edit).monitor, None);
}

#[test]
fn monitoring_an_unknown_track_is_an_error() {
    let (mut s, _p) = session(0);
    assert!(s.set_track_monitor(TrackId(999), None).is_err());
}

#[test]
fn levels_read_while_nothing_listens_to_the_channels() {
    let (mut s, mut p) = session(0);
    let (feed, mut w) = InputFeed::new(CHANNELS, BLOCK, 1);
    let status = feed.status();
    s.engine_mut().set_input(Some(feed)).unwrap();
    settled(&mut p, &mut w, [0.25, -0.5, 0.0], 512);
    assert_eq!(status.take_peak(0), 0.25);
    assert_eq!(status.take_peak(1), 0.5);
    assert_eq!(status.take_peak(2), 0.0);
}

#[test]
fn a_replaced_input_is_freed_off_the_audio_thread() {
    let (mut s, mut p) = session(0);
    let t = add_track(&mut s);
    s.set_track_monitor(t, Some(InputRoute::Mono(0))).unwrap();
    let mut first = connect(&mut s);
    settled(&mut p, &mut first, [0.5; 3], 256);
    s.engine().collect_garbage();

    let mut second = connect(&mut s);
    let out = settled(&mut p, &mut second, [0.25; 3], 4096);
    assert_close(out, [0.25, 0.25]);
    assert!(s.engine().collect_garbage() >= 1);

    s.engine_mut().set_input(None).unwrap();
    let out = render_offline(&mut p, 4096, 2);
    assert!(out[out.len() - 1].abs() < 1e-6, "silent once removed");
    assert!(s.engine().collect_garbage() >= 1);
}

#[test]
fn a_feed_with_shorter_blocks_than_the_engine_is_refused() {
    let (mut s, _p) = session(0);
    let (feed, _w) = InputFeed::new(2, BLOCK - 1, 1);
    assert!(matches!(
        s.engine_mut().set_input(Some(feed)),
        Err(EngineError::Incompatible(_))
    ));
}

#[test]
fn the_output_latency_is_published() {
    let (s, p) = session(0);
    assert_eq!(s.engine().output_latency_frames(), 0);
    p.set_output_latency_frames(480);
    assert_eq!(s.engine().output_latency_frames(), 480);
}
