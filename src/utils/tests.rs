use crate::utils::{display_name, format_bars, format_pan, format_time, snap_to_centre};

#[test]
fn test_parse_name() {
    let parsed = display_name("# This is a test n°#", 3);
    assert_eq!(parsed, "4 This is a test n°4")
}

#[test]
fn positions_count_bars_beats_and_sixteenths_from_one() {
    assert_eq!(format_bars(0., 4), "1.1.1");
    assert_eq!(format_bars(9.5, 4), "3.2.3");
    assert_eq!(format_bars(3.99, 4), "1.4.4");
    assert_eq!(format_bars(3., 3), "2.1.1");
    // Before the start: the bar before 1 is -1.
    assert_eq!(format_bars(-1., 4), "-1.4.1");
    assert_eq!(format_bars(-4.5, 4), "-2.4.3");
}

#[test]
fn times_round_to_the_millisecond() {
    assert_eq!(format_time(0.), "0:00.000");
    assert_eq!(format_time(83.25), "1:23.250");
    assert_eq!(format_time(2.9999998), "0:03.000");
    assert_eq!(format_time(-1.5), "-0:01.500");
}

#[test]
fn pans_read_as_sides_and_snap_to_centre() {
    assert_eq!(format_pan(0.), "C");
    assert_eq!(format_pan(0.004), "C");
    assert_eq!(format_pan(-0.35), "L35");
    assert_eq!(format_pan(1.), "R100");
    assert_eq!(snap_to_centre(0.01), 0.);
    assert_eq!(snap_to_centre(-0.2), -0.2);
}
