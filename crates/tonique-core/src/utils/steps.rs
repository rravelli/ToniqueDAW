//! How far the arrow keys move a value control once it's clicked: up and
//! right a step, down and left one back, a big step with Shift. Both apps
//! count the presses and step with these.

use tonique_engine::time::TimeSignature;

/// A control's step, and its big step (with Shift).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyStep {
    pub small: f64,
    pub big: f64,
}

impl KeyStep {
    pub const fn new(small: f64, big: f64) -> Self {
        Self { small, big }
    }
}

/// A gain, in dB.
pub const GAIN_DB: KeyStep = KeyStep::new(0.5, 6.);
/// A pan: 50 steps a side.
pub const PAN: KeyStep = KeyStep::new(1. / 50., 10. / 50.);
/// A time in a file (a clip's offset, its loop's position), in seconds.
pub const SECONDS: KeyStep = KeyStep::new(0.01, 1.);
/// A knob, as a share of its travel (even on log knobs).
pub const KNOB: KeyStep = KeyStep::new(0.01, 0.1);
/// A number field (the tempo, the time signature): units, tens with Shift.
pub const NUMBER: KeyStep = KeyStep::new(1., 10.);

/// A position or a length in beats: a sixteenth, a bar with Shift.
pub fn beats(signature: TimeSignature) -> KeyStep {
    KeyStep::new(0.25, signature.beats_per_bar())
}

/// The arrow keys pressed since last asked, each way netted out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Presses {
    /// Up or right, minus down or left.
    pub small: i32,
    /// The same with Shift.
    pub big: i32,
}

impl Presses {
    /// One press: `up` (or right) or back, `big` with Shift.
    pub fn press(&mut self, up: bool, big: bool) {
        let way = if up { 1 } else { -1 };
        if big {
            self.big += way;
        } else {
            self.small += way;
        }
    }

    /// One press, as [`Self::press`].
    pub fn one(up: bool, big: bool) -> Self {
        let mut presses = Self::default();
        presses.press(up, big);
        presses
    }

    pub fn is_empty(self) -> bool {
        self.small == 0 && self.big == 0
    }

    /// How far they go with `step`.
    pub fn amount(self, step: KeyStep) -> f64 {
        self.small as f64 * step.small + self.big as f64 * step.big
    }

    /// `value` moved by them with `step`, in `min..=max`.
    pub fn apply(self, value: f64, step: KeyStep, min: f64, max: f64) -> f64 {
        (value + self.amount(step)).clamp(min, max)
    }
}

/// A linear `gain` stepped in dB over `min_db..=max_db`, whose bottom is
/// silence (0): up from silence starts at the bottom.
pub fn step_gain(gain: f32, presses: Presses, min_db: f32, max_db: f32) -> f32 {
    let db = if gain > 0. {
        (20. * gain.log10()).clamp(min_db, max_db)
    } else {
        min_db
    };
    let db = presses.apply(db as f64, GAIN_DB, min_db as f64, max_db as f64) as f32;
    // Tenths, as it's shown, so steps from 0 dB land on 0.5, 1…
    let db = (db * 10.).round() / 10.;
    if db <= min_db {
        0.
    } else {
        10f32.powf(db / 20.)
    }
}

/// A `pan` stepped by fiftieths of a side, kept on them.
pub fn step_pan(pan: f32, presses: Presses) -> f32 {
    let stepped = presses.apply(pan as f64, PAN, -1., 1.);
    // `+ 0.` for no -0.
    ((stepped * 50.).round() / 50.) as f32 + 0.
}

/// A knob's position (0 to 1 of its travel) stepped.
pub fn step_ratio(ratio: f32, presses: Presses) -> f32 {
    presses.apply(ratio as f64, KNOB, 0., 1.) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(gain: f32) -> f32 {
        (20. * gain.log10() * 10.).round() / 10.
    }

    #[test]
    fn gains_step_in_decibels_from_silence() {
        let up = Presses::one(true, false);
        let down = Presses::one(false, false);
        let big_up = Presses::one(true, true);
        assert_eq!(db(step_gain(1., up, -40., 12.)), 0.5);
        assert_eq!(db(step_gain(1., big_up, -40., 12.)), 6.);
        assert_eq!(db(step_gain(0., up, -40., 12.)), -39.5);
        assert_eq!(step_gain(10f32.powf(-39.8 / 20.), down, -40., 12.), 0.);
        assert_eq!(db(step_gain(3.5, big_up, -40., 12.)), 12., "clamped");
    }

    #[test]
    fn pans_step_by_fiftieths_of_a_side() {
        let left = Presses::one(false, false);
        assert_eq!(step_pan(0., left), -0.02);
        assert_eq!(step_pan(0.3, Presses::one(true, true)), 0.5);
        assert_eq!(step_pan(-0.99, Presses::one(false, true)), -1.);
        assert_eq!(step_pan(0.02, left), 0.);
        assert!(step_pan(0.02, left).is_sign_positive());
    }

    #[test]
    fn presses_net_out() {
        let mut presses = Presses::default();
        presses.press(true, false);
        presses.press(true, true);
        presses.press(false, false);
        assert_eq!(presses, Presses { small: 0, big: 1 });
        assert_eq!(presses.amount(beats(TimeSignature::default())), 4.);
        assert!(!presses.is_empty());
        assert_eq!(step_ratio(0.95, presses), 1.);
    }
}
