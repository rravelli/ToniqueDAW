use egui::Rect;
use tonique_engine::{
    edit::{ClipId, ClipLoop, WarpMode},
    time::{BeatPos, TimeSignature},
};

use super::{inspector::*, markers::*, *};
use crate::{
    analysis::AudioInfo,
    core::{
        clip::{AudioClip, MAX_DETUNE, MIN_LENGTH},
        state::ProjectState,
    },
    utils::{format_bars, parse_bars},
};

/// A clip at beat 10 playing the middle half of a 4 s (8-beat, at 120
/// bpm) file: from 1 s, for 4 beats.
fn clip() -> AudioClip {
    let mut clip = AudioClip::new(ClipId(1), AudioInfo::silent(4.), BeatPos(10.), 120.);
    clip.offset = 1.;
    clip.length = 4.;
    clip
}

#[test]
fn trimming_the_start_keeps_the_audio_in_place() {
    let t = trimmed(&clip(), TrimEdge::Start, BeatPos(9.), 120.);
    assert_eq!((t.offset, t.position, t.length), (0.5, BeatPos(9.), 5.));
    // No further than the file's start...
    let t = trimmed(&clip(), TrimEdge::Start, BeatPos(5.), 120.);
    assert_eq!((t.offset, t.position), (0., BeatPos(8.)));
    // ...nor past the end, minus the minimum length.
    let t = trimmed(&clip(), TrimEdge::Start, BeatPos(15.), 120.);
    assert_eq!(t.length, MIN_LENGTH);
}

#[test]
fn trimming_the_start_stops_at_the_arrangement_start() {
    let mut near_zero = clip();
    near_zero.position = BeatPos(1.); // the file would start at beat -1
    let t = trimmed(&near_zero, TrimEdge::Start, BeatPos(-1.), 120.);
    assert_eq!((t.offset, t.position), (0.5, BeatPos::ZERO));
}

#[test]
fn trimming_the_end() {
    let t = trimmed(&clip(), TrimEdge::End, BeatPos(15.), 120.);
    assert_eq!((t.length, t.position, t.offset), (5., BeatPos(10.), 1.));
    // To the file's end, no further: 3 s left, 6 beats.
    assert_eq!(
        trimmed(&clip(), TrimEdge::End, BeatPos(28.), 120.).length,
        6.
    );
    assert_eq!(
        trimmed(&clip(), TrimEdge::End, BeatPos(8.), 120.).length,
        MIN_LENGTH
    );
}

#[test]
fn loops_stay_in_the_file_and_start_as_what_plays() {
    let looped = default_loop(&clip(), 120.);
    assert_eq!((looped.start_s, looped.end_s), (1., 3.));
    let c = with_loop(&clip(), -1., 10., 120.);
    let l = c.looped.unwrap();
    assert_eq!((l.start_s, l.end_s), (0., 4.), "inside the file");
    // Moving the loop before the start marker brings the marker in.
    let c = with_loop(&clip(), 0., 0.5, 120.);
    assert_eq!(c.offset, 0.);
    // Never shorter than the minimum.
    let l = with_loop(&clip(), 2., 2., 120.).looped.unwrap();
    assert!(l.end_s > l.start_s);
}

#[test]
fn typed_positions_and_lengths_read_as_shown() {
    const FOUR: TimeSignature = TimeSignature {
        numerator: 4,
        denominator: 4,
    };
    assert_eq!(parse_bars("3.2.3", FOUR), Some(9.5));
    assert_eq!(parse_bars("1.1.1", FOUR), Some(0.));
    assert_eq!(
        parse_bars("2", FOUR),
        Some(4.),
        "missing parts are the first"
    );
    assert_eq!(parse_bars("0.1.1", FOUR), None, "bars count from 1");
    assert_eq!(parse_bars("inf", FOUR), None);
    assert_eq!(parse_bars("1.NaN", FOUR), None);
    assert_eq!(parse_bars("1.2.3.4", FOUR), None);
    assert_eq!(parse_bars("x", FOUR), None);
    assert_eq!(parse_length("1.2.1", FOUR), Some(6.25));
    assert_eq!(parse_length("3", FOUR), Some(12.), "bars first");
    assert_eq!(parse_length("0.0.2", FOUR), Some(0.5));
    for beats in [0., 6.25, 9.5] {
        assert_eq!(parse_bars(&format_bars(beats, FOUR), FOUR), Some(beats));
    }
}

#[test]
fn lengths_count_bars_beats_and_sixteenths_from_zero() {
    const FOUR: TimeSignature = TimeSignature {
        numerator: 4,
        denominator: 4,
    };
    assert_eq!(format_length(4., FOUR), "1.0.0");
    assert_eq!(format_length(6.25, FOUR), "1.2.1");
    assert_eq!(format_length(3., FOUR), "0.3.0");
    for beats in [0.25, 6.25, 12.] {
        assert_eq!(parse_length(&format_length(beats, FOUR), FOUR), Some(beats));
    }
    // 6/8 counts eighths: a bar is 3 beats, of 6 eighths.
    let six_eight = TimeSignature::new(6, 8).unwrap();
    assert_eq!(format_length(3.5, six_eight), "1.1.0");
    assert_eq!(parse_length("1.1.0", six_eight), Some(3.5));
    assert_eq!(parse_bars("2.4.2", six_eight), Some(4.75))
}

#[test]
fn typed_times() {
    assert_eq!(parse_time("1:23.25"), Some(83.25));
    assert_eq!(parse_time("2.5"), Some(2.5));
    assert_eq!(parse_time("-1"), None);
}

/// Drives the view of one clip headless, with real pointer events.
struct Harness {
    ctx: egui::Context,
    details: ClipDetails,
    state: ProjectState,
    clip: ClipId,
    /// What the last frame drew.
    shapes: Vec<egui::epaint::ClippedShape>,
}

impl Harness {
    /// A 2 s (4-beat) file played from 0.5 s for 2 beats, at beat 5.
    fn new() -> Self {
        use tonique_engine::engine::{Engine, EngineConfig};
        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        let track = state.add_track();
        let clip = state.new_clip_id();
        let mut audio = AudioClip::new(clip, AudioInfo::silent(2.), BeatPos(5.), 120.);
        audio.offset = 0.5;
        audio.length = 2.;
        state.add_clips(&track, vec![audio]);
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let mut harness = Self {
            ctx,
            details: ClipDetails::new(),
            state,
            clip,
            shapes: Vec::new(),
        };
        harness.frame(Vec::new());
        harness
    }

    fn clip(&self) -> AudioClip {
        self.state.find_clip(self.clip).unwrap().1
    }

    fn frame(&mut self, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000., 200.),
            )),
            time: Some(self.ctx.input(|i| i.time) + 1.),
            events,
            ..Default::default()
        };
        let clip = self.clip();
        let (details, state) = (&mut self.details, &mut self.state);
        let mut output = self.ctx.run_ui(input, |ui| {
            let track = state.tracks().next().unwrap();
            details.ui(ui, &clip, &track, state);
        });
        output.textures_delta.clear();
        self.shapes = output.shapes;
    }

    /// Where the last frame wrote `text`.
    fn text(&self, text: &str) -> Rect {
        self.shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == text => {
                    Some(t.galley.rect.translate(t.pos.to_vec2()))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{text:?} isn't shown"))
    }

    /// A click, in one frame: frames are a second apart, too long for one.
    fn click(&mut self, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        self.frame(vec![
            egui::Event::PointerMoved(at),
            button(true),
            button(false),
        ]);
        self.frame(Vec::new());
    }

    /// Two clicks in a row, in one frame.
    fn double_click(&mut self, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let clicks = [true, false, true, false].map(button);
        self.frame([vec![egui::Event::PointerMoved(at)], clicks.to_vec()].concat());
        self.frame(Vec::new());
    }

    fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        self.frame(vec![egui::Event::PointerMoved(from), button(from, true)]);
        for step in 1..=4 {
            let at = from.lerp(to, step as f32 / 4.);
            self.frame(vec![egui::Event::PointerMoved(at)]);
        }
        self.frame(vec![button(to, false)]);
        self.frame(Vec::new());
    }

    /// The middle of the lane, `at` seconds into the file.
    fn at(&self, at: f64) -> egui::Pos2 {
        let lane = self.details.lane;
        egui::pos2(self.details.x(at, 120., lane), lane.center().y)
    }

    /// The loop's bracket, at the ruler's top, `at` seconds in.
    fn on_loop(&self, at: f64) -> egui::Pos2 {
        let lane = self.details.lane;
        let top = lane.top() - RULER_HEIGHT + LOOP_BAR_HEIGHT / 2.;
        egui::pos2(self.details.x(at, 120., lane), top)
    }
}

#[test]
fn dragging_the_end_marker_trims_the_clip_in_one_step() {
    let mut h = Harness::new();
    assert_eq!(h.clip().end(120.), BeatPos(7.)); // at 1.5 s in the file
    // Out past the file's end: it stops there, at 2 s.
    let (from, to) = (h.at(1.5), h.at(2.5));
    h.drag(from, to);
    assert_eq!(h.clip().length, 3.);
    h.state.undo();
    assert_eq!(h.clip().length, 2., "one step");
}

#[test]
fn the_start_marker_slips_the_audio_and_bar_one_stays() {
    let mut h = Harness::new();
    let file_start = h.at(0.).x;
    assert_eq!(h.details.origin, 0., "the file's start");
    let (from, to) = (h.at(0.5), h.at(0.75));
    h.drag(from, to);
    let clip = h.clip();
    assert_eq!((clip.offset, clip.position), (0.75, BeatPos(5.)), "slipped");
    assert_eq!(h.details.origin, 0., "bar 1 stayed");
    assert!((h.at(0.).x - file_start).abs() < 0.01, "the audio stayed");
}

#[test]
fn moving_bar_one_keeps_the_audio_in_place_and_is_undone() {
    let mut h = Harness::new();
    let file_start = h.at(0.).x;
    h.state.set_clip_origin(h.clip, 0.25);
    h.frame(Vec::new());
    assert_eq!(h.details.origin, 0.25);
    assert!((h.at(0.).x - file_start).abs() < 0.01, "the audio stayed");
    h.state.undo();
    h.frame(Vec::new());
    assert_eq!(h.details.origin, 0.);
}

#[test]
fn the_loop_brackets_end_drags_and_the_clip_outgrows_the_file() {
    let mut h = Harness::new();
    let mut looped = h.clip();
    looped.set_loop(
        Some(ClipLoop {
            start_s: 0.5,
            end_s: 1.5,
        }),
        120.,
    );
    looped.trim_end_at(BeatPos(20.), 120.);
    h.state.commit_resize_clip(looped);
    assert_eq!(h.clip().end(120.), BeatPos(20.), "past the file's end");
    h.frame(Vec::new());

    let (from, to) = (h.on_loop(1.5), h.on_loop(1.75));
    h.drag(from, to);
    let l = h.clip().looped.unwrap();
    assert_eq!((l.start_s, l.end_s), (0.5, 1.75));
    h.state.undo();
    assert_eq!(h.clip().looped.unwrap().end_s, 1.5, "one step");
}

#[test]
fn warping_keeps_the_view_as_it_was() {
    let mut h = Harness::new();
    h.details.grid.offset.x += 40.;
    let (zoom, offset) = (h.details.grid.beats_to_width(1.), h.details.grid.offset);
    let mut warped = h.clip();
    warped.set_warp(WarpMode::Spectral, 100., 0, 0., 120.);
    h.state.commit_resize_clip(warped);
    h.frame(Vec::new());
    assert_ne!(h.details.speed, 1., "played faster");
    assert_eq!(
        (h.details.grid.beats_to_width(1.), h.details.grid.offset),
        (zoom, offset)
    );
}

/// Dragged, a transposition follows the pointer: a quarter of the field's
/// width is a quarter of its range (12 of -24 to 24 semitones), one undo
/// step.
#[test]
fn dragging_the_transposition_follows_the_pointer() {
    let mut h = Harness::new();
    // Each cell's label is at its left.
    let left = h.text("Transpose").left();
    let width = h.text("Detune").left() - left - CELL_GAP;
    let from = h.text("+0 st").center();
    h.drag(from, from + egui::vec2(width / 4., 0.));
    assert_eq!(h.clip().transpose, 12);
    h.state.undo();
    assert_eq!(h.clip().transpose, 0);
}

/// The inspector's panels are laid out alike: their sections and rows
/// start at the same heights.
#[test]
fn the_clip_and_audio_panels_line_up() {
    let h = Harness::new();
    for (clip, audio) in [
        ("CLIP", "AUDIO"),
        ("Start", "Gain"),
        ("Length", "Transpose"),
        ("LOOP", "WARP"),
        ("Position", "Window"),
    ] {
        assert_eq!(h.text(clip).top(), h.text(audio).top(), "{clip} / {audio}");
    }
}

/// Detuned to its furthest, a clip stays at its transposition: a pitch
/// splits into semitones and cents one way only.
#[test]
fn detuning_to_the_end_keeps_the_transposition() {
    let mut h = Harness::new();
    let left = h.text("Detune").left();
    let from = h.text("+0 ct").center();
    // Well past the field's end, at once.
    h.drag(from, egui::pos2(left + 1000., from.y));
    assert_eq!((h.clip().transpose, h.clip().detune), (0, MAX_DETUNE));
}

/// Double-clicked, a transposition is back to none, one undo step.
#[test]
fn double_clicking_the_transposition_resets_it() {
    let mut h = Harness::new();
    let mut clip = h.clip();
    clip.transpose = 5;
    h.state.commit_resize_clip(clip);
    h.frame(Vec::new());
    let at = h.text("+5 st").center();
    h.double_click(at);
    assert_eq!(h.clip().transpose, 0);
    h.state.undo();
    assert_eq!(h.clip().transpose, 5);
}

/// The warp's title switches it off and on again.
#[test]
fn the_warp_title_switches_it() {
    let mut h = Harness::new();
    assert_eq!(h.clip().warp, WarpMode::Spectral);
    let title = h.text("WARP").center();
    h.click(title);
    assert_eq!(h.clip().warp, WarpMode::Off);
    h.click(title);
    assert_eq!(h.clip().warp, WarpMode::Spectral);
}

/// Stepped by the arrow keys past its range, the detune carries over into
/// the transposition, a cent at a time.
#[test]
fn detune_steps_carry_over_into_the_transposition() {
    let mut h = Harness::new();
    let mut clip = h.clip();
    clip.detune = MAX_DETUNE;
    h.state.commit_resize_clip(clip);
    h.frame(Vec::new());
    let at = h.text("+49 ct").center();
    h.click(at);
    let key = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    h.frame(vec![key(egui::Key::ArrowUp)]);
    assert_eq!((h.clip().transpose, h.clip().detune), (1, -50.));
    h.frame(vec![key(egui::Key::ArrowDown)]);
    assert_eq!((h.clip().transpose, h.clip().detune), (0, 49.));
}
