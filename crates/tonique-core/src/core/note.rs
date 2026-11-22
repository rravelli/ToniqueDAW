//! Pitches as notes, for tuners.

/// Note names from C, sharps for the black keys.
pub const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
/// Off by at most this many cents, a note is in tune.
pub const IN_TUNE: f32 = 3.;
/// A pitch as a note: its name, octave (A4 is 440 Hz by default) and how
/// far off it is, from -50 to 50 cents.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub name: &'static str,
    pub octave: i32,
    pub cents: f32,
}

impl Note {
    pub fn in_tune(&self) -> bool {
        self.cents.abs() <= IN_TUNE
    }
}

/// The note nearest `hz`, with A4 at `reference` Hz.
pub fn note(hz: f32, reference: f32) -> Note {
    let semis = 69. + 12. * (hz / reference).log2();
    let nearest = semis.round();
    let midi = nearest as i32;
    Note {
        name: NAMES[midi.rem_euclid(12) as usize],
        octave: midi.div_euclid(12) - 1,
        cents: (semis - nearest) * 100.,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitches_read_as_notes() {
        assert_eq!(
            note(440., 440.),
            Note {
                name: "A",
                octave: 4,
                cents: 0.
            }
        );
        let e2 = note(82.41, 440.);
        assert_eq!((e2.name, e2.octave), ("E", 2));
        assert!(e2.cents.abs() < 0.1);
        // B3 is just under C4: the octave turns over at C.
        assert_eq!(note(246.94, 440.).octave, 3);
        assert_eq!(note(261.63, 440.).octave, 4);
        // 10 cents sharp, and flat.
        let sharp = note(440. * (10f32 / 1200.).exp2(), 440.);
        assert_eq!(sharp.name, "A");
        assert!((sharp.cents - 10.).abs() < 0.01);
        let flat = note(440. * (-10f32 / 1200.).exp2(), 440.);
        assert!((flat.cents + 10.).abs() < 0.01);
        // Over 50 cents sharp is the next note, flat.
        assert_eq!(note(440. * (60f32 / 1200.).exp2(), 440.).name, "A#");
    }

    #[test]
    fn the_reference_moves_every_note() {
        let a = note(432., 432.);
        assert_eq!((a.name, a.octave), ("A", 4));
        assert!(a.cents.abs() < 0.01);
        assert!(note(440., 432.).cents > 30.);
    }
}
