//! The whole app, driven headless with `egui_kittest`: random input to find
//! panics and broken invariants, and screenshots to look at.
//!
//! Environment variables:
//! - `TONIQUE_FUZZ_SEED`: run this seed only (to replay a failure).
//! - `TONIQUE_FUZZ_STEPS`: how many inputs each seed gets.
//! - `TONIQUE_FUZZ_TRACE`: print each input once given, with the memory
//!   then in use, for failures that abort before they can tell the last ones.
//! - `TONIQUE_FUZZ_SHOT`: keep a screenshot of how a run ends.
//! - `TONIQUE_FUZZ_CHECK_EVERY`: how often the invariants are checked (see
//!   [`CHECK_EVERY`]).
//! - `TONIQUE_FUZZ_MAX_MB`: the memory the run may use (see [`MAX_MB`]).
//!
//! Screenshots, and one of the app when a run fails, go to
//! `target/ui-screenshots`.

use super::ToniqueApp;
use crate::{
    analysis::AudioInfo,
    audio::host::engine_without_audio,
    config::{config_dir, settings::Settings},
    core::clip::AudioClip,
    ui::font::fonts,
};
use egui::{Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, Rect, Vec2, pos2, vec2};
use egui_kittest::{Harness, kittest::NodeT};
use rand::{Rng, SeedableRng, rngs::StdRng, seq::IndexedRandom};
use std::{
    collections::VecDeque,
    f32::consts::TAU,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    path::{Path, PathBuf},
    sync::Arc,
};
use tonique_engine::time::BeatPos;

const SIZE: Vec2 = vec2(1400., 900.);
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

/// What the project holds when a run starts.
#[derive(Clone, Copy, Debug)]
enum Start {
    /// A few tracks with a clip each.
    Few,
    Empty,
    /// Many tracks, with clips back to back and overlapping.
    Many,
}

/// The app, as `main` opens it without an audio device, on a project of a few
/// tracks with clips and a browser folder of a few files.
fn app() -> Harness<'static, ToniqueApp> {
    app_with(Start::Few)
}

fn app_with(start: Start) -> Harness<'static, ToniqueApp> {
    let samples = samples_folder();
    let config = serde_json::json!({ "directories": [samples] });
    let dir = config_dir().unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.json"), config.to_string()).unwrap();

    let mut harness = Harness::builder().with_size(SIZE).build_eframe(|cc| {
        cc.egui_ctx.set_fonts(fonts());
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let settings = Settings::default();
        let engine = engine_without_audio(&settings);
        ToniqueApp::new(engine, Err("no audio in tests".into()), settings, cc)
    });
    let state = &mut harness.state_mut().state;
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
    harness.run_steps(4);
    harness
}

/// A folder of short sine waves (one in a subfolder, one mono), a file that
/// isn't audio and one that only pretends to be.
fn samples_folder() -> PathBuf {
    let dir = config_dir().unwrap().join("samples");
    std::fs::create_dir_all(dir.join("Pads")).unwrap();
    write_wav(&dir.join("kick.wav"), 0.3, 2, 60.);
    write_wav(&dir.join("loop.wav"), 2., 2, 220.);
    write_wav(&dir.join("mono.wav"), 1., 1, 440.);
    write_wav(&dir.join("Pads/pad.wav"), 4., 2, 110.);
    std::fs::write(dir.join("broken.wav"), b"RIFF not really").unwrap();
    std::fs::write(dir.join("notes.txt"), b"not audio").unwrap();
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
    for i in 0..frames {
        let sample = (i as f32 / RATE as f32 * hz * TAU).sin() * 0.5;
        for _ in 0..channels {
            wav.extend(((sample * i16::MAX as f32) as i16).to_le_bytes());
        }
    }
    std::fs::write(path, wav).unwrap();
}

/// Saved to `target/ui-screenshots/{name}.png`, if the app can be rendered
/// here (it needs a GPU adapter, or a software one).
fn screenshot(harness: &mut Harness<'_, ToniqueApp>, name: &str) -> Option<PathBuf> {
    let image = match harness.render() {
        Ok(image) => image,
        Err(e) => {
            eprintln!("Couldn't render {name}: {e}");
            return None;
        }
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ui-screenshots");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{name}.png"));
    image.save(&path).ok()?;
    Some(path)
}

/// One input to the app.
#[derive(Debug)]
enum Input {
    Click(Pos2, PointerButton, Modifiers),
    DoubleClick(Pos2),
    Drag(Pos2, Pos2, Modifiers),
    Scroll(Pos2, Vec2, Modifiers),
    Hover(Pos2),
    Key(Key, Modifiers),
    Text(&'static str),
    Copy,
    Cut,
    Paste(&'static str),
    /// Files dragged from the file manager, and dropped (or let go of
    /// elsewhere, outside the window).
    DropFiles(Pos2, Vec<&'static str>, bool),
    Resize(Vec2),
    /// Some time for the background (decoding, searching) to catch up.
    Wait,
}

/// Ctrl as Linux and Windows send it.
const CTRL: Modifiers = Modifiers::CTRL.plus(Modifiers::COMMAND);
const CTRL_SHIFT: Modifiers = CTRL.plus(Modifiers::SHIFT);

const KEYS: [Key; 24] = [
    Key::Escape,
    Key::Enter,
    Key::Space,
    Key::Delete,
    Key::Backspace,
    Key::Tab,
    Key::ArrowUp,
    Key::ArrowDown,
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::Home,
    Key::End,
    Key::PageUp,
    Key::PageDown,
    Key::F2,
    Key::F5,
    Key::Num0,
    Key::Num1,
    Key::Num3,
    Key::Num7,
    Key::A,
    Key::E,
    Key::Q,
    Key::W,
];

/// The app's shortcuts (as bound by default), and egui's zoom.
const SHORTCUTS: [(Modifiers, Key); 22] = [
    (CTRL, Key::L),
    (CTRL, Key::M),
    (CTRL_SHIFT, Key::F),
    (CTRL, Key::Z),
    (CTRL, Key::Y),
    (CTRL_SHIFT, Key::Z),
    (CTRL, Key::A),
    (CTRL, Key::D),
    (CTRL, Key::R),
    (CTRL, Key::K),
    (CTRL, Key::T),
    (CTRL, Key::G),
    (CTRL, Key::B),
    (CTRL, Key::N),
    (CTRL, Key::S),
    (CTRL, Key::J),
    (CTRL_SHIFT, Key::G),
    (CTRL, Key::Comma),
    (CTRL, Key::Plus),
    (CTRL, Key::Minus),
    (CTRL, Key::Num0),
    (Modifiers::SHIFT, Key::ArrowLeft),
];

const TEXTS: [&str; 14] = [
    "kick",
    "a",
    "Pads",
    "zzz",
    "",
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

/// The window's smallest is 900×560 (see `window.rs`), which at the
/// largest zoom (2.5) leaves the app 360×224 points.
const SIZES: [Vec2; 5] = [
    SIZE,
    vec2(1920., 1080.),
    vec2(900., 560.),
    vec2(600., 373.),
    vec2(360., 224.),
];

impl Input {
    /// Mostly on a widget the app shows (from its accessibility tree), else
    /// anywhere in the window (of `size`): the timeline and the waveforms
    /// are painted, not widgets.
    fn random(rng: &mut StdRng, widgets: &[Rect], size: Vec2) -> Self {
        let pos = |rng: &mut StdRng| match widgets.choose(rng) {
            Some(rect) if rng.random_bool(0.6) => pos2(
                rng.random_range(rect.left()..=rect.right()),
                rng.random_range(rect.top()..=rect.bottom()),
            ),
            _ => pos2(rng.random_range(0. ..size.x), rng.random_range(0. ..size.y)),
        };
        let modifiers = |rng: &mut StdRng| match rng.random_range(0..10) {
            0 => CTRL,
            1 => Modifiers::SHIFT,
            2 => Modifiers::ALT,
            3 => CTRL_SHIFT,
            _ => Modifiers::NONE,
        };
        match rng.random_range(0..100) {
            0..24 => Input::Click(pos(rng), PointerButton::Primary, modifiers(rng)),
            24..30 => Input::Click(pos(rng), PointerButton::Secondary, Modifiers::NONE),
            30..34 => Input::DoubleClick(pos(rng)),
            34..48 => Input::Drag(pos(rng), pos(rng), modifiers(rng)),
            48..54 => {
                let delta = vec2(rng.random_range(-3. ..3.), rng.random_range(-3. ..3.));
                Input::Scroll(pos(rng), delta, modifiers(rng))
            }
            54..59 => Input::Hover(pos(rng)),
            59..73 => Input::Key(*KEYS.choose(rng).unwrap(), modifiers(rng)),
            73..85 => {
                let (modifiers, key) = *SHORTCUTS.choose(rng).unwrap();
                Input::Key(key, modifiers)
            }
            85..89 => Input::Text(TEXTS.choose(rng).unwrap()),
            89 => Input::Copy,
            90 => Input::Cut,
            91 => Input::Paste(TEXTS.choose(rng).unwrap()),
            92..95 => {
                let n = rng.random_range(1..=3);
                let files = FILES.choose_multiple(rng, n).copied().collect();
                Input::DropFiles(pos(rng), files, rng.random_bool(0.85))
            }
            95..97 => Input::Resize(*SIZES.choose(rng).unwrap()),
            _ => Input::Wait,
        }
    }

    fn apply(&self, harness: &mut Harness<'_, ToniqueApp>) {
        let press = |harness: &mut Harness<'_, ToniqueApp>, pos, button, pressed, modifiers| {
            harness.event(Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers,
            });
            harness.step();
        };
        let to = |harness: &mut Harness<'_, ToniqueApp>, pos| {
            harness.event(Event::PointerMoved(pos));
            harness.step();
        };
        match self {
            &Input::Click(pos, button, modifiers) => {
                to(harness, pos);
                press(harness, pos, button, true, modifiers);
                press(harness, pos, button, false, modifiers);
            }
            &Input::DoubleClick(pos) => {
                to(harness, pos);
                for _ in 0..2 {
                    press(harness, pos, PointerButton::Primary, true, Modifiers::NONE);
                    press(harness, pos, PointerButton::Primary, false, Modifiers::NONE);
                }
            }
            &Input::Drag(from, until, modifiers) => {
                to(harness, from);
                press(harness, from, PointerButton::Primary, true, modifiers);
                for i in 1..=6 {
                    to(harness, from.lerp(until, i as f32 / 6.));
                }
                press(harness, until, PointerButton::Primary, false, modifiers);
            }
            &Input::Scroll(pos, delta, modifiers) => {
                to(harness, pos);
                harness.event(Event::MouseWheel {
                    unit: MouseWheelUnit::Line,
                    delta,
                    modifiers,
                    phase: egui::TouchPhase::Move,
                });
                harness.step();
            }
            &Input::Hover(pos) => to(harness, pos),
            &Input::Key(key, modifiers) => {
                harness.key_press_modifiers(modifiers, key);
                harness.step();
            }
            Input::Text(text) => {
                harness.event(Event::Text((*text).into()));
                harness.step();
            }
            Input::Copy => harness.event(Event::Copy),
            Input::Cut => harness.event(Event::Cut),
            Input::Paste(text) => harness.event(Event::Paste((*text).into())),
            Input::DropFiles(pos, files, dropped) => {
                let samples = config_dir().unwrap().join("samples");
                let hovered: Vec<_> = files
                    .iter()
                    .map(|file| egui::HoveredFile {
                        path: Some(samples.join(file)),
                        mime: String::new(),
                    })
                    .collect();
                let moves = [pos2(pos.x * 0.5, pos.y * 0.5), *pos];
                for at in moves {
                    harness.input_mut().hovered_files = hovered.clone();
                    to(harness, at);
                }
                harness.input_mut().hovered_files.clear();
                if *dropped {
                    harness.input_mut().dropped_files = files
                        .iter()
                        .map(|file| {
                            Arc::new(DroppedPath(samples.join(file))) as egui::DroppedFileHandle
                        })
                        .collect();
                } else {
                    harness.event(Event::PointerGone);
                }
                harness.step();
            }
            &Input::Resize(size) => {
                harness.set_size(size);
                harness.step();
            }
            Input::Wait => {
                std::thread::sleep(std::time::Duration::from_millis(30));
                harness.run_steps(3);
            }
        }
        harness.step();
    }
}

/// A file dropped from the file manager.
#[derive(Debug)]
struct DroppedPath(PathBuf);

impl egui::DroppedFile for DroppedPath {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// Where the app's widgets are, in the window.
fn widgets(harness: &Harness<'_, ToniqueApp>, size: Vec2) -> Vec<Rect> {
    let window = Rect::from_min_size(Pos2::ZERO, size);
    harness
        .root()
        .children_recursive()
        .filter_map(|node| node.accesskit_node().bounding_box())
        .map(|b| {
            let rect = Rect::from_min_max(
                pos2(b.x0 as f32, b.y0 as f32),
                pos2(b.x1 as f32, b.y1 as f32),
            );
            rect.intersect(window)
        })
        .filter(|rect| rect.is_positive())
        .collect()
}

/// What must hold whatever the user did: clips have a length and don't play
/// past their file, the project saves and opens again as it was, and
/// undoing everything then redoing it gets back to it.
fn check(harness: &mut Harness<'_, ToniqueApp>) {
    let state = &mut harness.state_mut().state;
    let bpm = state.bpm() as f64;
    for clip in state.tracks().flat_map(|t| t.clips) {
        assert!(clip.length > 0., "{:?} has no length", clip.id);
        let end = clip.offset + clip.length * 60. / bpm;
        assert!(
            clip.active_loop().is_some() || end <= clip.source_seconds() as f64 + 1e-6,
            "{:?} plays to {end} s of its {} s file",
            clip.id,
            clip.source_seconds()
        );
    }
    let project = state.project(None);
    let path = config_dir().unwrap().join("check.tonique");
    project.write(&path).expect("the project saves");
    let read = crate::core::project::ProjectFile::read(&path).expect("the project opens");
    assert_eq!(read, project, "the project opens as it was saved");

    // Redone as many as were undone: the user may have undone some already.
    // Each redo must bring back what was there before its undo.
    let could_redo = state.can_redo();
    let mut before = vec![project.clone()];
    while state.can_undo() {
        state.undo();
        before.push(state.project(None));
        assert!(before.len() < 10_000, "undo never runs out");
    }
    let undone = before.len() - 1;
    for i in 1..=undone {
        assert!(state.can_redo(), "redo {i} of the {undone} undone");
        state.redo();
        assert_eq!(
            state.project(None),
            before[undone - i],
            "redo {i} of the {undone} undone brings back what its undo took"
        );
    }
    assert_eq!(
        state.can_redo(),
        could_redo,
        "what was left to redo still is"
    );
    harness.run_steps(2);
}

/// `steps` random inputs from `seed`, checked as they go. A failure tells
/// the seed and the last inputs, and keeps a screenshot.
fn fuzz(seed: u64, steps: usize) {
    let mut rng = StdRng::seed_from_u64(seed);
    guard_memory();
    let start = [Start::Few, Start::Empty, Start::Many][seed as usize % 3];
    let mut harness = app_with(start);
    let mut size = SIZE;
    let mut recent = VecDeque::new();
    let trace = std::env::var_os("TONIQUE_FUZZ_TRACE").is_some();
    let check_every = env("TONIQUE_FUZZ_CHECK_EVERY").unwrap_or(CHECK_EVERY);
    let run = catch_unwind(AssertUnwindSafe(|| {
        for step in 0..steps {
            let input = Input::random(&mut rng, &widgets(&harness, size), size);
            if let Input::Resize(to) = input {
                size = to;
            }
            if recent.len() == 20 {
                recent.pop_front();
            }
            recent.push_back(format!("{step}: {input:?}"));
            input.apply(&mut harness);
            if trace {
                let (depth, last) = harness.state().state.undo_history();
                eprintln!(
                    "{step}: {input:?} ({} MB, undo {depth}: {last:?})",
                    resident_mb()
                );
            }
            if (step + 1) % check_every == 0 {
                check(&mut harness);
            }
        }
        check(&mut harness);
        if std::env::var_os("TONIQUE_FUZZ_SHOT").is_some() {
            screenshot(&mut harness, &format!("fuzz-{seed}-end"));
        }
    }));
    if let Err(panic) = run {
        eprintln!(
            "Seed {seed} failed (TONIQUE_FUZZ_SEED={seed} replays it), from {start:?}, after:"
        );
        for input in &recent {
            eprintln!("  {input}");
        }
        // Rendering a broken app may panic too.
        let shot = catch_unwind(AssertUnwindSafe(|| {
            screenshot(&mut harness, &format!("fuzz-{seed}"))
        }));
        if let Ok(Some(path)) = shot {
            eprintln!("The app then: {}", path.display());
        }
        resume_unwind(panic);
    }
}

/// Abort the process once it uses more than [`MAX_MB`]: an input that
/// sends the app allocating without end would otherwise take all the
/// machine's memory. Under a debugger, the abort stops where it's stuck.
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
                std::thread::sleep(std::time::Duration::from_millis(20));
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

/// Not a check: the app as it opens, to look at.
#[test]
#[ignore = "renders with the GPU; run with --ignored to look at the app"]
fn screenshots() {
    let mut harness = app();
    harness.run_steps(10);
    screenshot(&mut harness, "start");
}
