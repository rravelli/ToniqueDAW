use super::*;

#[test]
fn fitting_shows_exactly_the_range() {
    let mut grid = GridService::new();
    let viewport = Rect::from_min_size(egui::pos2(100., 0.), egui::vec2(400., 50.));
    grid.fit(BeatPos(8.), BeatPos(16.), 400.);
    assert_eq!(grid.pixels_per_beat(), 50.);
    assert_eq!(grid.beats_to_x(BeatPos(8.), viewport), 100.);
    assert_eq!(grid.beats_to_x(BeatPos(16.), viewport), 500.);
    assert_eq!(grid.line_step(), 0.5); // the lines follow the zoom

    // Before beat 0 only when allowed.
    grid.fit(BeatPos(-2.), BeatPos(6.), 400.);
    assert_eq!(grid.offset.x, 0.);
    let mut grid = GridService::new().with_negative_offset();
    grid.fit(BeatPos(-2.), BeatPos(6.), 400.);
    assert_eq!(grid.beats_to_x(BeatPos(-2.), viewport), 100.);
}

fn signature(numerator: u32, denominator: u32) -> TimeSignature {
    TimeSignature::new(numerator, denominator).unwrap()
}

/// A grid in `numerator`/`denominator` at `pixels_per_beat`.
fn grid(numerator: u32, denominator: u32, pixels_per_beat: f32) -> GridService {
    let mut grid = GridService::new();
    grid.set_time_signature(signature(numerator, denominator));
    grid.pixels_per_beat = pixels_per_beat;
    grid
}

/// Fixed to `step`.
fn fixed(mut grid: GridService, step: GridStep) -> GridService {
    grid.settings.adaptive = false;
    grid.settings.step = step;
    grid
}

fn lines(grid: &GridService, to: f64, step: f64) -> Vec<GridLine> {
    grid.lines(BeatPos(0.), BeatPos(to), step)
        .into_iter()
        .map(|(_, line)| line)
        .collect()
}

fn positions(grid: &GridService, to: f64, step: f64) -> Vec<f64> {
    grid.lines(BeatPos(0.), BeatPos(to), step)
        .into_iter()
        .map(|(at, _)| at.0)
        .collect()
}

#[test]
fn beats_and_bars_in_four_four() {
    let grid = grid(4, 4, 20.);
    assert_eq!(grid.line_step(), 1.);
    assert_eq!(
        lines(&grid, 4., 1.),
        [
            GridLine::Bar(1),
            GridLine::Beat { bar: 1, beat: 2 },
            GridLine::Beat { bar: 1, beat: 3 },
            GridLine::Beat { bar: 1, beat: 4 },
            GridLine::Bar(2),
        ]
    );
}

#[test]
fn adaptive_lines_follow_the_zoom_and_density() {
    let mut grid = grid(4, 4, 20.);
    let mut steps = Vec::new();
    for density in GridDensity::ALL {
        grid.settings.density = density;
        steps.push(grid.line_step());
    }
    assert_eq!(steps, [4., 1., 0.5]);
    // Zoomed out, lines span bars; in, thirty-seconds.
    grid.settings.density = GridDensity::Medium;
    grid.pixels_per_beat = 1.;
    assert_eq!(grid.line_step(), 32.);
    grid.pixels_per_beat = 1000.;
    assert_eq!(grid.line_step(), 0.125);
    // And the grid snaps to what it shows.
    assert_eq!(grid.snap_step(), 0.125);
}

/// 7/8 counts eighths: bars are 3.5 quarter notes.
#[test]
fn odd_meters_land_bars_between_quarters() {
    let grid = grid(7, 8, 50.);
    assert_eq!(grid.line_step(), 0.5);
    assert_eq!(
        grid.lines(BeatPos(3.), BeatPos(3.5), 0.5)
            .into_iter()
            .map(|(_, line)| line)
            .collect::<Vec<_>>(),
        [GridLine::Beat { bar: 1, beat: 7 }, GridLine::Bar(2)]
    );
    // Sixteenths: every other line is an eighth.
    assert_eq!(
        grid.lines(BeatPos(3.25), BeatPos(4.), 0.25)
            .into_iter()
            .map(|(_, line)| line)
            .collect::<Vec<_>>(),
        [
            GridLine::Division {
                bar: 1,
                beat: 7,
                sub: Some(2)
            },
            GridLine::Bar(2),
            GridLine::Division {
                bar: 2,
                beat: 1,
                sub: Some(2)
            },
            GridLine::Beat { bar: 2, beat: 2 },
        ]
    );
}

/// A fixed step that doesn't fit the bar starts over at each bar.
#[test]
fn steps_start_over_at_each_bar() {
    let grid = fixed(grid(7, 8, 50.), GridStep::Half);
    assert_eq!(
        positions(&grid, 7., grid.line_step()),
        [0., 2., 3.5, 5.5, 7.]
    );
    assert_eq!(grid.snap_to_step(BeatPos(3.)), BeatPos(3.5));
    assert_eq!(grid.snap_to_step(BeatPos(2.6)), BeatPos(2.));
}

/// Compound meters count their felt beats, three eighths each, then the
/// eighths in them.
#[test]
fn compound_meters_count_felt_beats() {
    let grid = grid(6, 8, 50.);
    assert_eq!(grid.pulse(), 1.5);
    assert_eq!(
        lines(&grid, 3., 0.5),
        [
            GridLine::Bar(1),
            GridLine::Division {
                bar: 1,
                beat: 1,
                sub: Some(2)
            },
            GridLine::Division {
                bar: 1,
                beat: 1,
                sub: Some(3)
            },
            GridLine::Beat { bar: 1, beat: 2 },
            GridLine::Division {
                bar: 1,
                beat: 2,
                sub: Some(2)
            },
            GridLine::Division {
                bar: 1,
                beat: 2,
                sub: Some(3)
            },
            GridLine::Bar(2),
        ]
    );
    // 3/8 is a simple meter: it counts its eighths.
    assert_eq!(self::grid(3, 8, 50.).pulse(), 0.5);
}

/// Zoomed out, each line spans several bars and is numbered by its bar.
#[test]
fn multi_bar_lines_number_their_bars() {
    let grid = grid(3, 4, 1.);
    assert_eq!(grid.line_step(), 24.);
    assert_eq!(
        lines(&grid, 48., 12.),
        [
            GridLine::Bar(1),
            GridLine::Bar(5),
            GridLine::Bar(9),
            GridLine::Bar(13),
            GridLine::Bar(17)
        ]
    );
}

#[test]
fn snapping_follows_the_signature() {
    let grid = fixed(grid(7, 8, 20.), GridStep::Bar);
    assert_eq!(grid.snap_to_step(BeatPos(4.0)), BeatPos(3.5));
    assert_eq!(grid.snap_to_step(BeatPos(5.5)), BeatPos(7.0));
}

/// A fixed grid keeps snapping to its step zoomed out too far to draw
/// it, showing coarser lines.
#[test]
fn fixed_steps_snap_at_any_zoom() {
    for pixels_per_beat in [0.5, 2., 10., 100., 2000.] {
        let grid = fixed(grid(4, 4, pixels_per_beat), GridStep::Sixteenth);
        assert_eq!(grid.snap_step(), 0.25);
        assert_eq!(grid.snap_to_step(BeatPos(10.3)), BeatPos(10.25));
        assert_eq!(grid.snap_to_step(BeatPos(10.4)), BeatPos(10.5));
        let line = grid.line_step();
        assert!(divides(line, 0.25), "{line} isn't a number of sixteenths");
        assert!(grid.beats_to_width(line) >= MIN_LINE_SPACING);
    }
    let grid = fixed(grid(4, 4, 0.5), GridStep::Sixteenth);
    assert!(grid.line_step() >= 4., "bars at least");
}

#[test]
fn triplets_split_beats_in_three() {
    let mut grid = fixed(grid(4, 4, 200.), GridStep::Eighth);
    grid.settings.triplet = true;
    let third = 1. / 3.;
    assert!((grid.snap_step() - third).abs() < 1e-9);
    let at = positions(&grid, 1., grid.line_step());
    assert_eq!(at.len(), 4);
    assert!(
        at.iter()
            .zip([0., third, 2. * third, 1.])
            .all(|(a, b)| (a - b).abs() < 1e-9)
    );
    assert_eq!(
        lines(&grid, 1., grid.line_step())[1..],
        [
            GridLine::Division {
                bar: 1,
                beat: 1,
                sub: Some(2)
            },
            GridLine::Division {
                bar: 1,
                beat: 1,
                sub: Some(3)
            },
            GridLine::Beat { bar: 1, beat: 2 },
        ]
    );
    assert!((grid.snap_to_step(BeatPos(0.3)).0 - third).abs() < 1e-9);
    // Sixteenth triplets: six to a beat.
    grid.settings.step = GridStep::Sixteenth;
    assert_eq!(positions(&grid, 1., grid.line_step()).len(), 7);
    // Adaptive: the triplets dividing the beat.
    grid.settings.adaptive = true;
    let step = grid.line_step();
    assert!(divides(1., step) && divides(step, 1. / 6.), "{step}");
}

#[test]
fn steps_are_named_as_notes_or_bars() {
    let grid = grid(4, 4, 20.);
    assert_eq!(grid.step_name(0.25), "1/16");
    assert_eq!(grid.step_name(2. / 3.), "1/4T");
    assert_eq!(grid.step_name(1. / 3.), "1/8T");
    assert_eq!(grid.step_name(4.), "1 bar");
    assert_eq!(grid.step_name(16.), "4 bars");
    let grid = self::grid(6, 8, 20.);
    assert_eq!(grid.step_name(grid.pulse()), "3/8");
    for step in GridStep::ALL {
        for triplet in [false, true] {
            let grid = self::grid(4, 4, 20.);
            assert_eq!(
                grid.step_name(step.beats(signature(4, 4), triplet)),
                step.name(triplet)
            );
        }
    }
}

#[test]
fn narrowing_and_widening() {
    let mut grid = grid(4, 4, 20.);
    grid.narrow();
    assert_eq!(grid.settings.density, GridDensity::Narrower);
    grid.widen();
    grid.widen();
    grid.widen();
    assert_eq!(grid.settings.density, GridDensity::Wider);
    // Fixed: the step it showed, then finer.
    grid.settings.density = GridDensity::Medium;
    grid.toggle_adaptive();
    assert!(!grid.settings.adaptive);
    assert_eq!(grid.settings.step, GridStep::Quarter);
    grid.narrow();
    assert_eq!(grid.settings.step, GridStep::Eighth);
    grid.toggle_adaptive();
    assert!(grid.settings.adaptive);
}

/// Before beat 0 bars count down from -1: there's no bar 0.
#[test]
fn bars_before_the_start_count_down_from_minus_one() {
    let grid = grid(4, 4, 20.);
    let lines = |step| {
        grid.lines(BeatPos(-8.), BeatPos(0.), step)
            .into_iter()
            .map(|(_, line)| line)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        lines(4.),
        [GridLine::Bar(-2), GridLine::Bar(-1), GridLine::Bar(1)]
    );
    assert_eq!(lines(1.)[7], GridLine::Beat { bar: -1, beat: 4 });
}

const SIGNATURES: [(u32, u32); 11] = [
    (2, 4),
    (3, 4),
    (4, 4),
    (5, 4),
    (7, 4),
    (2, 2),
    (6, 8),
    (7, 8),
    (12, 8),
    (5, 16),
    (32, 16),
];

/// Every zoom from the farthest out to the farthest in.
fn zooms() -> impl Iterator<Item = f32> {
    (0..=80).map(|n| 0.5 * 1.122_f32.powi(n))
}

/// Whatever the signature and zoom, labels fit between their lines and
/// sit on them.
#[test]
fn labels_never_overlap() {
    for (numerator, denominator) in SIGNATURES {
        for pixels_per_beat in zooms() {
            for adaptive in [true, false] {
                let mut grid = grid(numerator, denominator, pixels_per_beat);
                grid.settings.adaptive = adaptive;
                grid.settings.step = GridStep::ThirtySecond;
                let to = BeatPos(grid.width_to_beats(1500.) + 2000.);
                let step = grid.label_step(BeatPos(0.), to);
                let last = (to.0 / grid.beats_per_bar()) as i64 + 1;
                let at = format!("{numerator}/{denominator} at {pixels_per_beat}");
                assert!(
                    grid.beats_to_width(step) >= grid.label_width(step, last),
                    "{at}: labels {step} beats apart overlap"
                );
                let line = grid.line_step();
                assert!(
                    divides(step, line) || divides(step, grid.beats_per_bar()),
                    "{at}: labels {step} apart off the lines {line} apart"
                );
            }
        }
    }
}

/// Labels are as close as they fit: long bars aren't left with a label
/// every few bars, short ones get fewer.
#[test]
fn labels_go_by_the_room_between_lines() {
    // 7/4 bars 63 points wide: a label each.
    let grid = grid(7, 4, 9.);
    assert_eq!(grid.label_step(BeatPos(0.), BeatPos(100.)), 7.);
    // 2/4 bars 4 points wide: one in 8.
    let grid = self::grid(2, 4, 2.);
    assert_eq!(grid.label_step(BeatPos(0.), BeatPos(100.)), 16.);
    // Zoomed in, sub-beats are numbered.
    let grid = self::grid(4, 4, 400.);
    let step = grid.label_step(BeatPos(0.), BeatPos(2.));
    assert!(step < 1.);
    assert_eq!(
        grid.lines(BeatPos(1.), BeatPos(1.5), step)[2].1,
        GridLine::Division {
            bar: 1,
            beat: 2,
            sub: Some(3)
        }
    );
}

#[test]
fn time_labels_take_nice_steps() {
    let mut grid = grid(4, 4, 20.);
    // 40 points a second at 120.
    assert_eq!(grid.time_step(120.), 1.);
    grid.pixels_per_beat = 1.;
    assert_eq!(grid.time_step(120.), 30.);
    grid.pixels_per_beat = 2000.;
    let step = grid.time_step(120.);
    assert!(step < 1., "{step}: sub-second labels");
    for pixels_per_beat in zooms() {
        grid.pixels_per_beat = pixels_per_beat;
        let step = grid.time_step(120.);
        let width = step as f32 * pixels_per_beat * 2.;
        let chars = if step < 1. { 8. } else { 5. };
        assert!(
            width >= chars * LABEL_CHAR_WIDTH + LABEL_GAP,
            "{step} s at {pixels_per_beat}"
        );
    }
}
