//! The whole app, driven headless with gpui-fast's test platform: random
//! input to find panics and broken invariants, as the egui app's
//! `ui::app::tests` does. Each input is drawn after it's handled, so
//! rendering runs too; there's no renderer, hence no screenshots.
//!
//! Environment variables:
//! - `TONIQUE_FUZZ_SEED`: run this seed only (to replay a failure).
//! - `TONIQUE_FUZZ_STEPS`: how many inputs each seed gets.
//! - `TONIQUE_FUZZ_TRACE`: print each input once given, with the memory
//!   then in use, for failures that abort before they can tell the last ones.
//! - `TONIQUE_FUZZ_CHECK_EVERY`: how often the invariants are checked (see
//!   [`CHECK_EVERY`]).
//! - `TONIQUE_FUZZ_MAX_MB`: the memory the run may use (see [`MAX_MB`]).

use std::{
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    path::{Path, PathBuf},
    time::Duration,
};

use gpui::{
    Entity, ExternalPaths, FileDropEvent, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ScrollDelta, ScrollEvent, Size, TestAppContext, VisualTestContext,
    WindowHandle, point, px, size,
};
use tonique_core::{
    config::{config_dir, settings::Settings},
    device::host::Startup,
    document::{clip::AudioClip, file::ProjectFile},
    library::analysis::AudioInfo,
    project::ProjectState,
};
use tonique_engine::{
    edit::{MAX_SPEED, MIN_SPEED, WarpMode},
    time::BeatPos,
};

use crate::{app::Workspace, model::HoldsProject};

const SIZE: (f32, f32) = (1400., 820.);
/// Inputs per seed, unless `TONIQUE_FUZZ_STEPS` says otherwise.
const STEPS: usize = 300;
const SEEDS: u64 = 4;
/// Every so many inputs, the invariants are checked, unless
/// `TONIQUE_FUZZ_CHECK_EVERY` says otherwise (1 fails right after the input
/// that broke them).
const CHECK_EVERY: usize = 50;
/// The test process may use this much memory, unless `TONIQUE_FUZZ_MAX_MB`
/// says otherwise: past it, it aborts rather than take the machine down.
const MAX_MB: u64 = 1500;
/// The project's ticker waits this long when nothing plays: each input
/// lets that much time pass, so files load and the playhead moves.
const TICK: Duration = Duration::from_millis(100);

/// What the project holds when a run starts.
#[derive(Clone, Copy, Debug)]
enum Start {
    /// A few tracks with a clip each.
    Few,
    Empty,
    /// Many tracks, with clips back to back and overlapping.
    Many,
}

/// The app as `main` opens it, without an audio device, its browser
/// showing a folder of a few files.
fn open(start: Start) -> (VisualTestContext, Entity<Workspace>) {
    let folder = samples_folder();
    let config = format!("{{\"directories\": [{:?}]}}", folder.display().to_string());
    std::fs::write(
        config_dir().expect("a test config dir").join("config.json"),
        config,
    )
    .expect("the test config is written");

    let mut state = ProjectState::started(Startup::without_audio(Settings::default()));
    // Back to back, then overlapping.
    const MANY: &[(f32, f64)] = &[(1., 0.), (1., 2.), (0.5, 4.), (8., 3.)];
    let clips: &[&[(f32, f64)]] = match start {
        Start::Few => &[&[(2., 0.)], &[(4., 8.)], &[(1., 2.)]],
        Start::Empty => &[],
        Start::Many => &[MANY; 12],
    };
    for track_clips in clips {
        let track = state.add_track();
        for &(seconds, at) in *track_clips {
            let id = state.new_clip_id();
            let clip = AudioClip::new(id, AudioInfo::silent(seconds), BeatPos(at), 120.);
            state.add_clips(&track, vec![clip]);
        }
    }

    let mut app = TestAppContext::single();
    app.update(crate::theme::load_fonts);
    let window: WindowHandle<Workspace> =
        app.add_window(move |window, context| Workspace::new(state, window, context));
    let workspace = window.root(&mut app).expect("the workspace opens");
    let context = VisualTestContext::from_window(*window, &app);
    context.simulate_resize(size(px(SIZE.0), px(SIZE.1)));
    context.run_until_parked();
    (context, workspace)
}

/// A folder of short sine waves (one in a subfolder, one mono), a file that
/// isn't audio and one that only pretends to be.
fn samples_folder() -> PathBuf {
    let dir = config_dir().expect("a test config dir").join("samples");
    std::fs::create_dir_all(dir.join("Pads")).expect("the samples folder is made");
    write_wav(&dir.join("kick.wav"), 0.3, 2, 60.);
    write_wav(&dir.join("loop.wav"), 2., 2, 220.);
    write_wav(&dir.join("mono.wav"), 1., 1, 440.);
    write_wav(&dir.join("Pads/pad.wav"), 4., 2, 110.);
    std::fs::write(dir.join("broken.wav"), b"RIFF not really").expect("written");
    std::fs::write(dir.join("notes.txt"), b"not audio").expect("written");
    dir
}

/// 16-bit PCM at 48 kHz.
fn write_wav(path: &Path, seconds: f32, channels: u16, hz: f32) {
    const RATE: u32 = 48_000;
    let frames = (seconds * RATE as f32) as u32;
    let bytes = frames * channels as u32 * 2;
    let mut wav = Vec::with_capacity(44 + bytes as usize);
    wav.extend(b"RIFF");
    wav.extend((36 + bytes).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(channels.to_le_bytes());
    wav.extend(RATE.to_le_bytes());
    wav.extend((RATE * channels as u32 * 2).to_le_bytes());
    wav.extend((channels * 2).to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(bytes.to_le_bytes());
    for frame in 0..frames {
        let sample = (frame as f32 / RATE as f32 * hz * std::f32::consts::TAU).sin();
        let value = (sample * 0.5 * i16::MAX as f32) as i16;
        for _ in 0..channels {
            wav.extend(value.to_le_bytes());
        }
    }
    std::fs::write(path, wav).expect("the wav is written");
}

/// One input to the app.
#[derive(Debug)]
enum Input {
    Click(Point<Pixels>, MouseButton, Modifiers, usize),
    /// Pressed at the first point, moved through the others, let go at the last.
    Drag(Vec<Point<Pixels>>, Modifiers),
    Scroll(Point<Pixels>, Point<Pixels>, Modifiers),
    Hover(Point<Pixels>),
    /// As gpui writes them: `ctrl-z`, `escape`.
    Keys(&'static str),
    Text(&'static str),
    /// Files dragged from the file manager, and dropped (or let go of
    /// elsewhere, outside the window).
    DropFiles(Point<Pixels>, Vec<&'static str>, bool),
    Resize(Size<Pixels>),
    /// Some time for the background (decoding, searching) to catch up.
    Wait,
}

const KEYS: [&str; 24] = [
    "escape",
    "enter",
    "space",
    "delete",
    "backspace",
    "tab",
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "pageup",
    "pagedown",
    "f2",
    "f5",
    "0",
    "1",
    "3",
    "7",
    "a",
    "e",
    "q",
    "w",
];

/// The app's shortcuts, as bound by default.
const SHORTCUTS: [&str; 22] = [
    "ctrl-l",
    "ctrl-m",
    "ctrl-shift-f",
    "ctrl-z",
    "ctrl-y",
    "ctrl-shift-z",
    "ctrl-a",
    "ctrl-d",
    "ctrl-r",
    "ctrl-k",
    "ctrl-t",
    "ctrl-g",
    "ctrl-b",
    "ctrl-n",
    "ctrl-s",
    "ctrl-j",
    "ctrl-shift-g",
    "ctrl-,",
    "ctrl-shift-p",
    "shift-left",
    "alt-up",
    "ctrl-c ctrl-v",
];

const TEXTS: [&str; 13] = [
    "kick",
    "a",
    "Pads",
    "zzz",
    "0",
    "-1",
    "1e9",
    "NaN",
    "999999999999",
    "0.0001",
    "🎵 ünï",
    "a very long name that keeps going and going well past any column",
    "line\nbreak",
];

/// In the samples folder, or not there at all.
const FILES: [&str; 8] = [
    "kick.wav",
    "loop.wav",
    "mono.wav",
    "Pads/pad.wav",
    "Pads",
    "broken.wav",
    "notes.txt",
    "gone.wav",
];

const SIZES: [(f32, f32); 4] = [SIZE, (1920., 1080.), (900., 560.), (600., 373.)];

/// A small, fast generator: seeded runs replay exactly.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        // SplitMix64.
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut mixed = self.0;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        mixed ^ (mixed >> 31)
    }

    fn below(&mut self, count: usize) -> usize {
        (self.next() % count as u64) as usize
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn chance(&mut self, odds: f32) -> bool {
        self.unit() < odds
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

impl Input {
    /// Anywhere in the window (of `window`), often on the lanes, where the
    /// clips are, or near where the pointer last was: menus open under it,
    /// handles are a few pixels wide.
    fn random(random: &mut Random, window: Size<Pixels>, last: Point<Pixels>) -> Self {
        let (width, height) = (f32::from(window.width), f32::from(window.height));
        let position = |random: &mut Random| {
            let roll = random.unit();
            if roll < 0.35 {
                // The lanes, where the clips are: right of the browser and
                // the track list, between the transport and the bottom panel.
                point(
                    px(width * (0.3 + random.unit() * 0.7)),
                    px(height * (0.12 + random.unit() * 0.45)),
                )
            } else if roll < 0.6 {
                point(
                    (f32::from(last.x) + (random.unit() - 0.5) * 80.).clamp(0., width - 1.),
                    (f32::from(last.y) + (random.unit() - 0.5) * 80.).clamp(0., height - 1.),
                )
                .map(px)
            } else {
                point(px(random.unit() * width), px(random.unit() * height))
            }
        };
        let modifiers = |random: &mut Random| match random.below(10) {
            0 => Modifiers {
                shift: true,
                ..Default::default()
            },
            1 => Modifiers {
                control: true,
                ..Default::default()
            },
            2 => Modifiers {
                alt: true,
                ..Default::default()
            },
            _ => Modifiers::default(),
        };
        match random.below(100) {
            0..30 => {
                let button = if random.chance(0.2) {
                    MouseButton::Right
                } else {
                    MouseButton::Left
                };
                let count = if random.chance(0.15) { 2 } else { 1 };
                Input::Click(position(random), button, modifiers(random), count)
            }
            30..48 => {
                let from = position(random);
                let mut points = vec![from];
                let to = position(random);
                let moves = 1 + random.below(5);
                for index in 1..=moves {
                    let along = index as f32 / moves as f32;
                    points.push(point(
                        from.x + (to.x - from.x) * along,
                        from.y + (to.y - from.y) * along,
                    ));
                }
                Input::Drag(points, modifiers(random))
            }
            48..56 => {
                let delta = point(
                    px((random.unit() - 0.5) * 400.),
                    px((random.unit() - 0.5) * 400.),
                );
                Input::Scroll(position(random), delta, modifiers(random))
            }
            56..64 => Input::Hover(position(random)),
            64..76 => Input::Keys(random.pick(&KEYS)),
            76..86 => Input::Keys(random.pick(&SHORTCUTS)),
            86..91 => Input::Text(random.pick(&TEXTS)),
            91..94 => {
                let count = 1 + random.below(3);
                let files = (0..count).map(|_| random.pick(&FILES)).collect();
                Input::DropFiles(position(random), files, random.chance(0.8))
            }
            94..96 => {
                let (width, height) = random.pick(&SIZES);
                Input::Resize(size(px(width), px(height)))
            }
            _ => Input::Wait,
        }
    }

    /// Where the pointer ends up.
    fn apply(&self, context: &mut VisualTestContext, pointer: Point<Pixels>) -> Point<Pixels> {
        let pointer = match self {
            Input::Click(at, button, modifiers, count) => {
                context.simulate_event(MouseMoveEvent {
                    position: *at,
                    pressed_button: None,
                    modifiers: *modifiers,
                });
                for click in 1..=*count {
                    context.simulate_event(MouseDownEvent {
                        button: *button,
                        position: *at,
                        modifiers: *modifiers,
                        click_count: click,
                        first_mouse: false,
                    });
                    context.simulate_event(MouseUpEvent {
                        button: *button,
                        position: *at,
                        modifiers: *modifiers,
                        click_count: click,
                    });
                }
                *at
            }
            Input::Drag(points, modifiers) => {
                let from = points[0];
                context.simulate_event(MouseMoveEvent {
                    position: from,
                    pressed_button: None,
                    modifiers: *modifiers,
                });
                context.simulate_event(MouseDownEvent {
                    button: MouseButton::Left,
                    position: from,
                    modifiers: *modifiers,
                    click_count: 1,
                    first_mouse: false,
                });
                for &through in &points[1..] {
                    context.simulate_event(MouseMoveEvent {
                        position: through,
                        pressed_button: Some(MouseButton::Left),
                        modifiers: *modifiers,
                    });
                    context.executor().advance_clock(Duration::from_millis(16));
                    context.run_until_parked();
                }
                let to = *points.last().expect("a drag has points");
                context.simulate_event(MouseUpEvent {
                    button: MouseButton::Left,
                    position: to,
                    modifiers: *modifiers,
                    click_count: 1,
                });
                to
            }
            Input::Scroll(at, delta, modifiers) => {
                context.simulate_event(ScrollEvent {
                    position: *at,
                    delta: ScrollDelta::Pixels(*delta),
                    modifiers: *modifiers,
                    ..Default::default()
                });
                *at
            }
            Input::Hover(at) => {
                context.simulate_event(MouseMoveEvent {
                    position: *at,
                    pressed_button: None,
                    modifiers: Modifiers::default(),
                });
                *at
            }
            Input::Keys(keys) => {
                context.simulate_keystrokes(keys);
                pointer
            }
            Input::Text(text) => {
                context.simulate_input(text);
                pointer
            }
            Input::DropFiles(at, files, dropped) => {
                let folder = config_dir().expect("a test config dir").join("samples");
                let paths = ExternalPaths(files.iter().map(|file| folder.join(file)).collect());
                context.simulate_event(FileDropEvent::Entered {
                    position: *at,
                    paths,
                });
                context.simulate_event(FileDropEvent::Pending { position: *at });
                if *dropped {
                    context.simulate_event(FileDropEvent::Submit { position: *at });
                } else {
                    context.simulate_event(FileDropEvent::Exited);
                }
                *at
            }
            Input::Resize(to) => {
                context.simulate_resize(*to);
                pointer
            }
            Input::Wait => {
                for _ in 0..5 {
                    std::thread::sleep(Duration::from_millis(20));
                    context.executor().advance_clock(TICK);
                    context.run_until_parked();
                }
                pointer
            }
        };
        context.executor().advance_clock(TICK);
        context.run_until_parked();
        pointer
    }
}

/// The project's state, read or changed between inputs.
fn with_state<R>(
    context: &mut VisualTestContext,
    workspace: &Entity<Workspace>,
    change: impl FnOnce(&mut ProjectState) -> R,
) -> R {
    context.update(|_, app| {
        let project = workspace.read(app).project.clone();
        let mut state = project.state_mut(app);
        change(&mut state)
    })
}

/// What must hold whatever the user did, as in the egui app's fuzzing:
/// clips have a length and don't play past their file (but warped ones
/// the tempo is too far from their file's to stretch to: they go on
/// silent), the project saves and opens again as it was, and undoing
/// everything then redoing it gets back to it.
fn check(context: &mut VisualTestContext, workspace: &Entity<Workspace>) {
    with_state(context, workspace, |state| {
        let bpm = state.bpm() as f64;
        for clip in state.tracks().flat_map(|track| track.clips) {
            assert!(clip.length > 0., "{:?} has no length", clip.id);
            let end = clip.offset + clip.file_seconds(clip.length, bpm as f32);
            let ratio = bpm / clip.bpm;
            let stretched_too_far =
                clip.warp == WarpMode::Spectral && !(MIN_SPEED..=MAX_SPEED).contains(&ratio);
            assert!(
                clip.active_loop().is_some()
                    || stretched_too_far
                    || end <= clip.source_seconds() as f64 + 1e-6,
                "{:?} plays to {end} s of its {} s file",
                clip.id,
                clip.source_seconds()
            );
        }
        let project = state.project(None);
        let path = config_dir()
            .expect("a test config dir")
            .join("check.tonique");
        project.write(&path).expect("the project saves");
        let read = ProjectFile::read(&path).expect("the project opens");
        assert_eq!(read, project, "the project opens as it was saved");

        // Redone as many as were undone: the user may have undone some
        // already. Each redo must bring back what was there before its undo.
        let could_redo = state.can_redo();
        let mut before = vec![project.clone()];
        while state.can_undo() {
            state.undo();
            before.push(state.project(None));
            assert!(before.len() < 10_000, "undo never runs out");
        }
        let undone = before.len() - 1;
        for redo in 1..=undone {
            assert!(state.can_redo(), "redo {redo} of the {undone} undone");
            state.redo();
            assert_eq!(
                state.project(None),
                before[undone - redo],
                "redo {redo} of the {undone} undone brings back what its undo took"
            );
        }
        assert_eq!(
            state.can_redo(),
            could_redo,
            "what was left to redo still is"
        );
    });
    context.executor().advance_clock(TICK);
    context.run_until_parked();
}

/// `steps` random inputs from `seed`, checked as they go. A failure tells
/// the seed and the last inputs.
fn fuzz(seed: u64, steps: usize) {
    guard_memory();
    let mut random = Random(seed);
    let start = [Start::Few, Start::Empty, Start::Many][seed as usize % 3];
    let (mut context, workspace) = open(start);
    let mut window = size(px(SIZE.0), px(SIZE.1));
    let mut pointer = point(px(SIZE.0 / 2.), px(SIZE.1 / 2.));
    let mut recent = VecDeque::new();
    let trace = std::env::var_os("TONIQUE_FUZZ_TRACE").is_some();
    let check_every = env("TONIQUE_FUZZ_CHECK_EVERY").unwrap_or(CHECK_EVERY);
    let run = catch_unwind(AssertUnwindSafe(|| {
        for step in 0..steps {
            let input = Input::random(&mut random, window, pointer);
            if let Input::Resize(to) = input {
                window = to;
            }
            if recent.len() == 20 {
                recent.pop_front();
            }
            recent.push_back(format!("{step}: {input:?}"));
            pointer = input.apply(&mut context, pointer);
            if trace {
                let (depth, last) =
                    with_state(&mut context, &workspace, |state| state.undo_history());
                eprintln!(
                    "{step}: {input:?} ({} MB, undo {depth}: {last:?})",
                    resident_mb()
                );
            }
            if (step + 1) % check_every == 0 {
                check(&mut context, &workspace);
            }
        }
        check(&mut context, &workspace);
    }));
    if let Err(panic) = run {
        eprintln!(
            "Seed {seed} failed (TONIQUE_FUZZ_SEED={seed} replays it), from {start:?}, after:"
        );
        for input in &recent {
            eprintln!("  {input}");
        }
        resume_unwind(panic);
    }
}

/// Abort the process once it uses more than [`MAX_MB`]: an input that
/// sends the app allocating without end would otherwise take all the
/// machine's memory.
fn guard_memory() {
    static GUARD: std::sync::Once = std::sync::Once::new();
    let max = env("TONIQUE_FUZZ_MAX_MB").unwrap_or(MAX_MB);
    GUARD.call_once(|| {
        std::thread::spawn(move || {
            loop {
                let used = resident_mb();
                if used > max {
                    eprintln!("The test uses {used} MB, past {max}: aborting.");
                    std::process::abort();
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
    });
}

/// The test's resident memory (Linux only; 0 elsewhere).
fn resident_mb() -> u64 {
    let pages = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|statm| statm.split_whitespace().nth(1)?.parse::<u64>().ok());
    pages.unwrap_or(0) * 4096 / (1 << 20)
}

fn env<T: std::str::FromStr>(name: &str) -> Option<T> {
    std::env::var(name).ok()?.parse().ok()
}

#[test]
fn random_input_breaks_nothing() {
    let steps = env("TONIQUE_FUZZ_STEPS").unwrap_or(STEPS);
    match env("TONIQUE_FUZZ_SEED") {
        Some(seed) => fuzz(seed, steps),
        None => (0..SEEDS).for_each(|seed| fuzz(seed, steps)),
    }
}
