//! Takes through the whole engine, with a fake input device.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tonique_engine::edit::commands::AddTrack;
use tonique_engine::edit::{Edit, EditSession, Plugin, PluginKind, Track};
use tonique_engine::engine::{AudioProcessor, Engine, EngineConfig, EngineError, render_offline};
use tonique_engine::input::{InputFeed, InputRoute, InputWriter};
use tonique_engine::record::{CountIn, RecordOptions, RecordTrack, TakeReport};
use tonique_engine::sample::SampleBuffer;
use tonique_engine::time::{BeatPos, TempoMap};

const SR: f64 = 48000.0;
const BLOCK: usize = 128;
/// At 120 bpm.
const BEAT: usize = 24000;

fn session() -> (EditSession, AudioProcessor) {
    let (engine, processor) = Engine::new(EngineConfig {
        sample_rate: SR,
        max_block: BLOCK,
        housekeeping_thread: false,
        ..Default::default()
    });
    let session = EditSession::new(Edit::new(120.0), engine).expect("new session");
    (session, processor)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("tonique-take-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn options(dir: &Path, route: InputRoute) -> RecordOptions {
    RecordOptions {
        directory: dir.to_path_buf(),
        tracks: vec![RecordTrack {
            track: 1,
            route,
            name: "Track".into(),
        }],
        latency_offset_frames: 0,
        count_in: None,
    }
}

/// The audio's way from the output back to the input: `delay` frames, the
/// devices reporting `reported` of them.
struct Loopback {
    writer: InputWriter,
    /// Frames on their way, left channel only.
    line: Vec<f32>,
    delay: usize,
}

impl Loopback {
    fn connect(s: &mut EditSession, p: &AudioProcessor, input: usize, output: usize) -> Self {
        let (feed, writer) = InputFeed::new(1, BLOCK, BLOCK * 4);
        s.engine_mut().set_input(Some(feed)).expect("set input");
        writer.set_latency_frames(input);
        p.set_output_latency_frames(output);
        Self {
            writer,
            line: vec![0.0; input + output],
            delay: input + output,
        }
    }

    /// Run a device callback of `BLOCK` frames.
    fn callback(&mut self, p: &mut AudioProcessor) -> Vec<f32> {
        let mut out = vec![0.0; BLOCK * 2];
        p.process_interleaved(&mut out, 2);
        self.line.extend(out.iter().step_by(2).copied());
        let arrived: Vec<f32> = self.line.drain(..BLOCK).collect();
        debug_assert!(self.line.len() == self.delay);
        self.writer.push(arrived.into_iter());
        out
    }

    fn run(&mut self, p: &mut AudioProcessor, frames: usize) {
        for _ in 0..frames.div_ceil(BLOCK) {
            self.callback(p);
        }
    }
}

/// Run callbacks until the take's report is in.
fn finish(s: &mut EditSession, mut callback: impl FnMut()) -> TakeReport {
    let started = Instant::now();
    loop {
        callback();
        s.engine().collect_garbage();
        if let Some(report) = s.engine_mut().take_report() {
            assert!(report.error.is_none(), "{:?}", report.error);
            return report;
        }
        assert!(started.elapsed() < Duration::from_secs(20), "no report");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn read(path: &Path) -> Vec<f32> {
    SampleBuffer::load_wav(path, SR)
        .expect("readable take")
        .channel(0)
        .to_vec()
}

/// A click starts from silence: its first sample is 0 and the next isn't.
fn assert_click_at(samples: &[f32], frame: usize) {
    assert!(
        samples[frame - 10..=frame].iter().all(|s| *s == 0.0) && samples[frame + 1] != 0.0,
        "no click at {frame}: {:?}",
        &samples[frame - 3..frame + 3]
    );
}

#[allow(clippy::unwrap_used)]
fn loopback_takes_land_on_their_beats(name: &str, input: usize, output: usize, plugin: usize) {
    let dir = TempDir::new(name);
    let (mut s, mut p) = session();
    s.edit().metronome.set(1.0, 0.0);
    if plugin > 0 {
        // Delays the whole mix, click included, to keep it in time.
        let mut track = s.create(|e| Track::new(e, "latent"));
        let latent = s.create(|e| Plugin::new(e, PluginKind::Latency { samples: plugin }));
        track.channel.plugins.push(latent);
        s.perform(AddTrack::new(track)).unwrap();
    }
    let mut loopback = Loopback::connect(&mut s, &p, input, output);
    loopback.run(&mut p, BEAT / 4);

    s.engine_mut()
        .record_start(options(&dir.0, InputRoute::Mono(0)))
        .unwrap();
    s.play().unwrap();
    loopback.run(&mut p, 3 * BEAT);
    s.stop().unwrap();
    s.engine_mut().record_stop().unwrap();
    let report = finish(&mut s, || {
        loopback.callback(&mut p);
    });

    assert_eq!(report.dropouts, 0);
    assert_eq!(report.passes.len(), 1);
    let pass = &report.passes[0];
    assert_eq!(pass.start, 0);
    let samples = read(&pass.files[0].path);
    assert_eq!(samples.len() as u64, pass.frames);
    assert_click_at(&samples, BEAT);
    assert_click_at(&samples, 2 * BEAT);
}

#[test]
fn a_click_played_back_into_the_input_lands_on_its_beat() {
    loopback_takes_land_on_their_beats("loopback", 0, 0, 0);
}

#[test]
fn device_latencies_are_compensated() {
    loopback_takes_land_on_their_beats("devices", 700, 1300, 0);
}

#[test]
fn graph_latency_is_compensated() {
    loopback_takes_land_on_their_beats("graph", 100, 200, 1000);
}

#[test]
fn the_offset_moves_the_take() {
    let dir = TempDir::new("offset");
    let (mut s, mut p) = session();
    s.edit().metronome.set(1.0, 0.0);
    let mut loopback = Loopback::connect(&mut s, &p, 0, 0);
    loopback.run(&mut p, BEAT / 4);
    let mut opts = options(&dir.0, InputRoute::Mono(0));
    opts.latency_offset_frames = -100;
    s.engine_mut().record_start(opts).unwrap();
    s.play().unwrap();
    loopback.run(&mut p, 2 * BEAT);
    s.engine_mut().record_stop().unwrap();
    let report = finish(&mut s, || {
        loopback.callback(&mut p);
    });
    let samples = read(&report.passes[0].files[0].path);
    assert_click_at(&samples, BEAT + 100);
}

#[test]
fn looping_makes_a_file_per_pass_each_on_its_beats() {
    let dir = TempDir::new("loop");
    let (mut s, mut p) = session();
    s.edit().metronome.set(1.0, 0.0);
    let mut loopback = Loopback::connect(&mut s, &p, 300, 500);
    loopback.run(&mut p, BEAT / 4);

    s.engine_mut()
        .set_loop(Some((BEAT as i64, 3 * BEAT as i64)))
        .unwrap();
    s.engine_mut().seek(BEAT as i64 / 2).unwrap();
    s.engine_mut()
        .record_start(options(&dir.0, InputRoute::Mono(0)))
        .unwrap();
    s.play().unwrap();
    // Half a beat before the loop, two rounds and a bit.
    loopback.run(&mut p, BEAT / 2 + 4 * BEAT + BEAT / 2);
    s.engine_mut().record_stop().unwrap();
    s.stop().unwrap();
    let report = finish(&mut s, || {
        loopback.callback(&mut p);
    });

    assert_eq!(report.passes.len(), 3, "{report:?}");
    for pass in &report.passes {
        assert_eq!(pass.start, BEAT as i64);
    }
    assert_eq!(report.passes[0].frames, 2 * BEAT as u64);
    assert_eq!(report.passes[1].frames, 2 * BEAT as u64);
    assert!(report.passes[2].frames < 2 * BEAT as u64, "partial");
    let names: Vec<_> = report
        .passes
        .iter()
        .map(|p| p.files[0].path.file_name().unwrap().to_owned())
        .collect();
    assert_eq!(
        names,
        ["Track take 1.wav", "Track take 2.wav", "Track take 3.wav"]
    );
    for pass in &report.passes[..2] {
        let samples = read(&pass.files[0].path);
        // Beat 2 of the timeline, one beat into the file.
        assert_click_at(&samples, BEAT);
    }
}

/// Frames counted from 1, each value marking its frame.
fn numbered_input(writer: &mut InputWriter, next: &mut u32) {
    let frames: Vec<f32> = (0..BLOCK as u32)
        .flat_map(|i| {
            let n = (*next + i) as f32;
            [n, -n, n * 2.0]
        })
        .collect();
    *next += BLOCK as u32;
    writer.push(frames.into_iter());
}

#[test]
fn punching_in_and_out_while_playing_keeps_the_transport_going() {
    let dir = TempDir::new("punch");
    let (mut s, mut p) = session();
    let (feed, mut w) = InputFeed::new(3, BLOCK, BLOCK);
    let status = feed.status();
    s.engine_mut().set_input(Some(feed)).unwrap();
    let mut next = 1;
    s.play().unwrap();
    for _ in 0..100 {
        numbered_input(&mut w, &mut next);
        render_offline(&mut p, BLOCK, 2);
    }
    let latency = status.mean_fill_frames();
    assert_eq!(latency, BLOCK);

    s.engine_mut()
        .record_start(options(&dir.0, InputRoute::Stereo(1)))
        .unwrap();
    let punch_in = s.engine().position();
    for _ in 0..50 {
        numbered_input(&mut w, &mut next);
        render_offline(&mut p, BLOCK, 2);
    }
    s.engine_mut().record_stop().unwrap();
    let report = finish(&mut s, || {
        numbered_input(&mut w, &mut next);
        render_offline(&mut p, BLOCK, 2);
    });
    assert!(s.engine().is_playing(), "the transport goes on");

    assert_eq!(report.latency, latency as u64);
    assert_eq!(report.passes.len(), 1);
    let pass = &report.passes[0];
    assert_eq!(pass.start, punch_in);
    assert_eq!(pass.frames, 50 * BLOCK as u64, "as long as it was on");

    // In stereo, from the right input channels, gapless.
    let buffer = SampleBuffer::load_wav(&pass.files[0].path, SR).unwrap();
    assert_eq!(buffer.num_channels(), 2);
    let first = -buffer.channel(0)[0];
    for i in 0..buffer.len() {
        let n = first + i as f32;
        assert_eq!(buffer.channel(0)[i], -n, "frame {i}");
        assert_eq!(buffer.channel(1)[i], n * 2.0, "frame {i}");
    }
}

#[test]
fn each_track_gets_its_own_channels() {
    let dir = TempDir::new("tracks");
    let (mut s, mut p) = session();
    let (feed, mut w) = InputFeed::new(3, BLOCK, BLOCK);
    s.engine_mut().set_input(Some(feed)).unwrap();
    let mut next = 1;
    let mut opts = options(&dir.0, InputRoute::Mono(2));
    opts.tracks.push(RecordTrack {
        track: 2,
        route: InputRoute::Mono(0),
        name: "Other".into(),
    });
    s.engine_mut().record_start(opts).unwrap();
    s.play().unwrap();
    for _ in 0..100 {
        numbered_input(&mut w, &mut next);
        render_offline(&mut p, BLOCK, 2);
    }
    s.engine_mut().record_stop().unwrap();
    let report = finish(&mut s, || {
        numbered_input(&mut w, &mut next);
        render_offline(&mut p, BLOCK, 2);
    });
    let files = &report.passes[0].files;
    assert_eq!((files[0].track, files[1].track), (1, 2));
    assert!(files[1].path.ends_with("Other take 1.wav"));
    let (a, b) = (read(&files[0].path), read(&files[1].path));
    assert_eq!(a.len(), b.len());
    assert!(b[0] > 0.0);
    assert_eq!(a[0], b[0] * 2.0);
}

#[test]
fn channels_the_device_lacks_are_silent() {
    let dir = TempDir::new("missing");
    let (mut s, mut p) = session();
    let (feed, mut w) = InputFeed::new(3, BLOCK, BLOCK);
    s.engine_mut().set_input(Some(feed)).unwrap();
    let mut next = 1;
    s.engine_mut()
        .record_start(options(&dir.0, InputRoute::Mono(9)))
        .unwrap();
    s.play().unwrap();
    for _ in 0..100 {
        numbered_input(&mut w, &mut next);
        render_offline(&mut p, BLOCK, 2);
    }
    s.engine_mut().record_stop().unwrap();
    let report = finish(&mut s, || {
        numbered_input(&mut w, &mut next);
        render_offline(&mut p, BLOCK, 2);
    });
    let samples = read(&report.passes[0].files[0].path);
    assert!(samples.iter().all(|s| *s == 0.0));
}

#[test]
fn passes_too_short_to_be_meant_are_dropped() {
    let dir = TempDir::new("short");
    let (mut s, mut p) = session();
    let mut loopback = Loopback::connect(&mut s, &p, 0, 0);
    loopback.run(&mut p, BEAT / 4);
    s.engine_mut()
        .record_start(options(&dir.0, InputRoute::Mono(0)))
        .unwrap();
    s.play().unwrap();
    loopback.run(&mut p, BLOCK * 8); // well under 100 ms
    s.engine_mut().record_stop().unwrap();
    let report = finish(&mut s, || {
        loopback.callback(&mut p);
    });
    assert_eq!(report.passes, []);
    assert_eq!(
        std::fs::read_dir(&dir.0).unwrap().count(),
        0,
        "no file left"
    );
}

#[test]
fn a_count_in_clicks_with_the_transport_stopped_then_rolls() {
    let dir = TempDir::new("count-in");
    let (mut s, mut p) = session();
    let mut loopback = Loopback::connect(&mut s, &p, 0, 0);
    loopback.run(&mut p, BEAT / 4);

    // At this tempo the bar doesn't end on a callback's boundary.
    let tempo = TempoMap::new(110.0);
    let beat = 60.0 / 110.0 * SR;
    let bar = (4.0 * beat).round() as usize;
    assert_ne!(bar % BLOCK, 0);
    let mut opts = options(&dir.0, InputRoute::Mono(0));
    opts.count_in = CountIn::new(&tempo, BeatPos(0.0), 1, SR, 0.5);
    s.engine_mut().record_start(opts).unwrap();

    let mut heard = Vec::new();
    for _ in 0..bar / BLOCK {
        heard.extend(loopback.callback(&mut p).into_iter().step_by(2));
        assert!(s.engine().is_counting_in());
        assert!(!s.engine().is_playing());
        assert_eq!(s.engine().position(), 0);
    }
    for i in 0..4 {
        let at = (i as f64 * beat).round() as usize;
        assert!(
            heard[at + 1].abs() > 0.0 && heard[at + 1].abs() <= 0.5,
            "beat {i}"
        );
    }

    // The bar ends inside this callback, and it rolls right there.
    let rest = bar % BLOCK;
    loopback.callback(&mut p);
    assert!(!s.engine().is_counting_in());
    assert!(s.engine().is_playing());
    assert_eq!(s.engine().position(), (BLOCK - rest) as i64);

    loopback.run(&mut p, BEAT);
    s.engine_mut().record_stop().unwrap();
    let report = finish(&mut s, || {
        loopback.callback(&mut p);
    });
    assert_eq!(report.passes.len(), 1);
    assert_eq!(report.passes[0].start, 0);
    // The count-in came back into the input, and isn't part of the take.
    let samples = read(&report.passes[0].files[0].path);
    assert!(samples[..BEAT / 2].iter().all(|s| *s == 0.0));
}

#[test]
fn stopping_during_the_count_in_cancels_the_roll_and_records_nothing() {
    let dir = TempDir::new("cancel");
    let (mut s, mut p) = session();
    let mut loopback = Loopback::connect(&mut s, &p, 0, 0);
    let tempo = TempoMap::new(120.0);
    let mut opts = options(&dir.0, InputRoute::Mono(0));
    opts.count_in = CountIn::new(&tempo, BeatPos(0.0), 1, SR, 0.5);
    s.engine_mut().record_start(opts).unwrap();
    loopback.run(&mut p, BEAT);
    assert!(s.engine().is_counting_in());

    s.stop().unwrap();
    s.engine_mut().record_stop().unwrap();
    let report = finish(&mut s, || {
        loopback.callback(&mut p);
    });
    assert_eq!(report.passes, []);
    assert!(!s.engine().is_counting_in());
    loopback.run(&mut p, 2 * 4 * BEAT);
    assert!(!s.engine().is_playing(), "never rolls");
}

#[test]
fn a_count_in_is_ignored_while_playing() {
    let dir = TempDir::new("no-count-in");
    let (mut s, mut p) = session();
    let mut loopback = Loopback::connect(&mut s, &p, 0, 0);
    s.play().unwrap();
    loopback.run(&mut p, BEAT / 4);
    let tempo = TempoMap::new(120.0);
    let mut opts = options(&dir.0, InputRoute::Mono(0));
    opts.count_in = CountIn::new(&tempo, BeatPos(0.0), 1, SR, 0.5);
    s.engine_mut().record_start(opts).unwrap();
    loopback.run(&mut p, BEAT / 4);
    assert!(!s.engine().is_counting_in());
    s.engine_mut().record_stop().unwrap();
    finish(&mut s, || {
        loopback.callback(&mut p);
    });
}

#[test]
fn a_take_needs_tracks_and_an_input_and_only_one_runs_at_a_time() {
    let dir = TempDir::new("refused");
    let (mut s, mut p) = session();
    assert!(matches!(
        s.engine_mut()
            .record_start(options(&dir.0, InputRoute::Mono(0))),
        Err(EngineError::Record(_))
    ));
    let mut loopback = Loopback::connect(&mut s, &p, 0, 0);
    let mut none = options(&dir.0, InputRoute::Mono(0));
    none.tracks.clear();
    assert!(matches!(
        s.engine_mut().record_start(none),
        Err(EngineError::Record(_))
    ));

    assert!(!s.engine().is_recording());
    s.engine_mut()
        .record_start(options(&dir.0, InputRoute::Mono(0)))
        .unwrap();
    assert!(s.engine().is_recording());
    assert!(matches!(
        s.engine_mut()
            .record_start(options(&dir.0, InputRoute::Mono(0))),
        Err(EngineError::Record(_))
    ));
    s.engine_mut().record_stop().unwrap();
    finish(&mut s, || {
        loopback.callback(&mut p);
    });
    assert!(!s.engine().is_recording());
}

#[test]
fn a_directory_that_cannot_be_made_is_refused_up_front() {
    let dir = TempDir::new("no-dir");
    std::fs::write(&dir.0, b"a file in the way").unwrap();
    let (mut s, p) = session();
    let _loopback = Loopback::connect(&mut s, &p, 0, 0);
    let result = s
        .engine_mut()
        .record_start(options(&dir.0.join("sub"), InputRoute::Mono(0)));
    assert!(matches!(result, Err(EngineError::Record(_))));
    let _ = std::fs::remove_file(&dir.0);
}

#[test]
fn losing_the_engine_still_finishes_the_take() {
    let dir = TempDir::new("lost");
    let (mut s, mut p) = session();
    let mut loopback = Loopback::connect(&mut s, &p, 0, 0);
    loopback.run(&mut p, BEAT / 4);
    s.engine_mut()
        .record_start(options(&dir.0, InputRoute::Mono(0)))
        .unwrap();
    s.play().unwrap();
    loopback.run(&mut p, BEAT);

    drop(p); // the audio stream goes away with its processor
    let report = finish(&mut s, || {});
    assert_eq!(report.passes.len(), 1);
    assert!(report.passes[0].frames > 0);
    assert!(!read(&report.passes[0].files[0].path).is_empty());
}

#[test]
fn a_disk_failure_ends_the_take_with_a_report() {
    let dir = TempDir::new("disk");
    let (mut s, mut p) = session();
    let mut loopback = Loopback::connect(&mut s, &p, 0, 0);
    loopback.run(&mut p, BEAT / 4);
    s.engine_mut()
        .record_start(options(&dir.0, InputRoute::Mono(0)))
        .unwrap();
    // The folder goes away before the first file is made.
    std::fs::remove_dir_all(&dir.0).unwrap();
    s.play().unwrap();
    loopback.run(&mut p, BEAT);
    s.engine_mut().record_stop().unwrap();

    let started = Instant::now();
    let report = loop {
        loopback.callback(&mut p);
        s.engine().collect_garbage();
        if let Some(report) = s.engine_mut().take_report() {
            break report;
        }
        assert!(started.elapsed() < Duration::from_secs(20), "no report");
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(report.error.is_some());
    assert_eq!(report.passes, []);
}

/// What was written about a second ago survives a crash.
#[test]
fn a_take_is_playable_while_it_is_being_written() {
    let dir = TempDir::new("flush");
    let (mut s, mut p) = session();
    let mut loopback = Loopback::connect(&mut s, &p, 0, 0);
    loopback.run(&mut p, BEAT / 4);
    s.engine_mut()
        .record_start(options(&dir.0, InputRoute::Mono(0)))
        .unwrap();
    s.play().unwrap();
    loopback.run(&mut p, BEAT);

    let path = dir.0.join("Track take 1.wav");
    let started = Instant::now();
    loop {
        if path.exists() && SampleBuffer::load_wav(&path, SR).is_ok_and(|b| !b.is_empty()) {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "never flushed");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(s.engine().is_recording(), "still going");

    s.engine_mut().record_stop().unwrap();
    finish(&mut s, || {
        loopback.callback(&mut p);
    });
}

/// Sound in the room, in real time: a soft pulse (well below any Nyquist)
/// each time the transport, rolling at `roll` seconds, reaches a beat. What
/// an input at any rate records of it can be compared to the timeline.
fn room(t: f64, roll: f64, beat: f64) -> f32 {
    const WIDTH: f64 = 0.001;
    let since = t - roll;
    if since < -5.0 * WIDTH {
        return 0.0;
    }
    let nearest = (since / beat).round() * beat;
    let x = (since - nearest) / WIDTH;
    (-0.5 * x * x).exp() as f32
}

/// Where a pulse near `around` peaks, in frames, to a fraction of a frame.
fn pulse_at(samples: &[f32], around: usize) -> f64 {
    let window = 200;
    let range = around - window..around + window;
    let weight: f64 = samples[range.clone()].iter().map(|s| *s as f64).sum();
    let moment: f64 = range.map(|i| i as f64 * samples[i] as f64).sum();
    moment / weight
}

/// A device with a clock of its own, capturing the room.
struct RoomInput {
    writer: InputWriter,
    real_rate: f64,
    captured: usize,
    /// When the transport rolled, once it has.
    roll: Option<f64>,
    callbacks: usize,
}

impl RoomInput {
    /// In a size sharing no factor with `BLOCK`: its deliveries fall at every
    /// point of the output's callbacks, as those of a real device do.
    const BLOCK: usize = 479;

    fn now(&self) -> f64 {
        (self.callbacks * BLOCK) as f64 / SR
    }

    fn run(&mut self, p: &mut AudioProcessor, callbacks: usize) {
        let beat = BEAT as f64 / SR;
        for _ in 0..callbacks {
            let delivered = self.now() * self.real_rate;
            while ((self.captured + Self::BLOCK) as f64) <= delivered {
                let (captured, real_rate, roll) = (self.captured, self.real_rate, self.roll);
                self.writer.push((0..Self::BLOCK).map(|i| {
                    let t = (captured + i) as f64 / real_rate;
                    roll.map_or(0.0, |roll| room(t, roll, beat))
                }));
                self.captured += Self::BLOCK;
            }
            render_offline(p, BLOCK, 2);
            self.callbacks += 1;
            if self.callbacks.is_multiple_of(200) {
                std::thread::sleep(Duration::from_millis(1)); // for the writer
            }
        }
    }
}

/// Record the room for `seconds` from an input with a clock of its own, and
/// return how far each pulse landed from its beat, in engine frames.
#[allow(clippy::unwrap_used)]
fn recorded_pulse_errors(name: &str, nominal: f64, real: f64, seconds: f64) -> Vec<(f64, f64)> {
    let dir = TempDir::new(name);
    let (mut s, mut p) = session();
    let target = RoomInput::BLOCK + (BLOCK as f64 * nominal / SR).ceil() as usize;
    let (feed, writer) = InputFeed::resampled(1, BLOCK, target, nominal, SR);
    let status = feed.status();
    s.engine_mut().set_input(Some(feed)).unwrap();
    let mut input = RoomInput {
        writer,
        real_rate: real,
        captured: 0,
        roll: None,
        callbacks: 0,
    };
    input.run(&mut p, 600); // settled, as an input opened a while before

    s.engine_mut()
        .record_start(options(&dir.0, InputRoute::Mono(0)))
        .unwrap();
    s.play().unwrap();
    input.roll = Some(input.now());
    input.run(&mut p, (seconds * SR) as usize / BLOCK);
    s.engine_mut().record_stop().unwrap();
    s.stop().unwrap();
    let report = finish(&mut s, || input.run(&mut p, 1));

    assert_eq!(report.dropouts, 0, "{status:?}");
    assert_eq!(report.passes.len(), 1);
    assert_eq!(report.passes[0].start, 0);
    let samples = read(&report.passes[0].files[0].path);
    (1..)
        .map(|n| n * BEAT)
        .take_while(|at| at + 1000 < samples.len())
        .map(|at| (at as f64 / SR, pulse_at(&samples, at) - at as f64))
        .collect()
}

fn assert_on_the_beat(errors: &[(f64, f64)], tolerance: f64) {
    assert!(errors.len() > 3);
    for (time, error) in errors {
        assert!(error.abs() < tolerance, "{error} frames off at {time} s");
    }
}

#[test]
fn takes_at_another_rate_land_on_their_beats() {
    for (name, rate) in [("44k", 44100.0), ("96k", 96000.0), ("22k", 22050.0)] {
        let errors = recorded_pulse_errors(name, rate, rate, 5.0);
        assert_on_the_beat(&errors, 5.0);
    }
}

/// Two clocks: well within a millisecond (48 frames) from the start, as the
/// correction finds the clocks' difference, and no drift over a long take.
#[test]
fn takes_from_a_separate_clock_stay_on_their_beats() {
    for (name, ppm) in [("fast", 100.0), ("slow", -100.0)] {
        let real = SR * (1.0 + ppm * 1e-6);
        let errors = recorded_pulse_errors(name, SR, real, 120.0);
        assert_on_the_beat(&errors, 8.0);
        let settled: Vec<_> = errors.iter().filter(|e| e.0 > 30.0).copied().collect();
        assert_on_the_beat(&settled, 1.5);
    }
}
