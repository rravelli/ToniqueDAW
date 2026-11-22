//! A file's tempo, from the period that repeats most in its onset curve
//! ([`crate::onset`]): its autocorrelation, scored over each period's
//! multiples and weighted toward common tempos. Short loops are fitted to
//! a whole number of bars, which settles most halved or doubled tempos.
//!
//! Wrong tempos are worse than none: a clip placed at its file's tempo is
//! warped to the project's, so a wrong one changes how it sounds, where
//! none plays it as recorded. Hence the [`MIN_CONFIDENCE`].

use rustfft::{FftPlanner, num_complex::Complex};

use crate::onset::OnsetCurve;

/// The tempos looked for, in BPM.
pub const MIN_BPM: f64 = 50.;
pub const MAX_BPM: f64 = 220.;
/// Tempos are weighted by how far they are from this one, in octaves
/// (a log-normal of this width): 80–160 BPM come first.
const LIKELIEST_BPM: f64 = 120.;
const LIKELIEST_WIDTH_OCTAVES: f64 = 1.;
/// Shorter files are one-shots: no tempo.
const MIN_SECONDS: f64 = 1.5;
/// How much of the curve has to repeat at the tempo's period (0 to 1).
/// Below it, there's no steady beat.
pub const MIN_CONFIDENCE: f32 = 0.2;
/// The curve is compared with its average over this long around each
/// value: only what stands out of it counts.
const AVERAGE_SECONDS: f64 = 0.5;
/// Width of the blur over the curve (see [`smooth`]), in values.
const SMOOTHING_WIDTH: f64 = 1.5;
/// Multiples of a period scored along with it.
const MULTIPLES: usize = 4;
/// Periods tried, in values apart.
const PERIOD_STEP: f64 = 0.1;
/// The period is refined on its multiples up to this one, for precision.
const REFINE_UP_TO: usize = 32;
/// Files up to this long are fitted to whole bars.
const LOOP_SECONDS: f64 = 30.;
/// Beats a loop may hold: 1, 2, 4 or 8 bars of 4/4.
const LOOP_BEATS: [f64; 4] = [4., 8., 16., 32.];
/// How well a tempo fitting whole bars has to score, against the best
/// period's score, to be the loop's.
const LOOP_SCORE: f64 = 0.7;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoEstimate {
    pub bpm: f64,
    /// How much of the curve repeats at the tempo's period, from
    /// [`MIN_CONFIDENCE`] to 1.
    pub confidence: f32,
}

/// The tempo of the file with onsets `curve`, if it has a steady beat.
pub fn estimate(curve: &OnsetCurve) -> Option<TempoEstimate> {
    let rate = curve.rate();
    let seconds = curve.frames() as f64 / curve.sample_rate();
    if rate <= 0. || seconds < MIN_SECONDS {
        return None;
    }
    let mut novelty = smooth(&above_average(
        curve.values(),
        (rate * AVERAGE_SECONDS).round() as usize,
    ));
    // Centred, so that the correlation of unrelated values is 0.
    let mean = novelty.iter().sum::<f64>() / novelty.len().max(1) as f64;
    novelty.iter_mut().for_each(|value| *value -= mean);
    let correlation = autocorrelation(&novelty);
    let energy = *correlation.first()?;
    if energy <= f64::EPSILON {
        return None;
    }
    let shortest = ((rate * 60. / MAX_BPM).floor() as usize).max(1);
    let longest = ((rate * 60. / MIN_BPM).ceil() as usize).min(novelty.len() / 2);
    if longest <= shortest {
        return None;
    }
    let likeliness = |period: f64| {
        let octaves = (60. * rate / period / LIKELIEST_BPM).log2() / LIKELIEST_WIDTH_OCTAVES;
        (-0.5 * octaves * octaves).exp()
    };
    let score = |period: f64| {
        (1..=MULTIPLES)
            .filter_map(|multiple| {
                let lag = (period * multiple as f64).round() as usize;
                Some(peak_near(&correlation, lag)? / multiple as f64)
            })
            .sum::<f64>()
            * likeliness(period)
    };
    // Periods rarely fall on a whole number of values: their multiples
    // would miss their peaks.
    let steps = ((longest - shortest) as f64 / PERIOD_STEP) as usize;
    let best = (0..=steps)
        .map(|step| shortest as f64 + step as f64 * PERIOD_STEP)
        .max_by(|a, b| score(*a).total_cmp(&score(*b)))?;
    // A loop's tempo fits a whole number of bars in it: the likeliest of
    // those, if it repeats nearly as well as the best period. Exact, as
    // the loop's length is.
    let fitted = (seconds <= LOOP_SECONDS)
        .then(|| {
            bar_tempos(seconds)
                .map(|bpm| (bpm, score(60. * rate / bpm)))
                .max_by(|a, b| a.1.total_cmp(&b.1))
        })
        .flatten()
        .filter(|(_, fit)| *fit >= LOOP_SCORE * score(best));
    let (bpm, period) = match fitted {
        Some((bpm, _)) => (bpm, 60. * rate / bpm),
        None => {
            let period = refine(&correlation, best);
            (60. * rate / period, period)
        }
    };
    let confidence =
        (peak_near(&correlation, period.round() as usize)? / energy).clamp(0., 1.) as f32;
    (confidence >= MIN_CONFIDENCE).then_some(TempoEstimate { bpm, confidence })
}

/// How far each value stands above its average over `width` values around
/// it (0 when below).
fn above_average(values: &[f32], width: usize) -> Vec<f64> {
    let half = width / 2;
    let mut sums = Vec::with_capacity(values.len() + 1);
    sums.push(0.);
    for value in values {
        sums.push(sums.last().copied().unwrap_or(0.) + *value as f64);
    }
    (0..values.len())
        .map(|index| {
            let from = index.saturating_sub(half);
            let to = (index + half + 1).min(values.len());
            let average = (sums[to] - sums[from]) / (to - from) as f64;
            (values[index] as f64 - average).max(0.)
        })
        .collect()
}

/// `values` blurred over a few of them: a hit's value is often a single
/// one, and a period between two whole numbers of values would split its
/// correlation between them.
fn smooth(values: &[f64]) -> Vec<f64> {
    let radius = (3. * SMOOTHING_WIDTH).ceil() as usize;
    let kernel: Vec<f64> = (0..=2 * radius)
        .map(|index| {
            let distance = (index as f64 - radius as f64) / SMOOTHING_WIDTH;
            (-0.5 * distance * distance).exp()
        })
        .collect();
    let total: f64 = kernel.iter().sum();
    (0..values.len())
        .map(|index| {
            kernel
                .iter()
                .enumerate()
                .filter_map(|(offset, weight)| {
                    let at = (index + offset).checked_sub(radius)?;
                    Some(values.get(at)? * weight)
                })
                .sum::<f64>()
                / total
        })
        .collect()
}

/// Autocorrelation of `values` for every lag up to half their length, each
/// averaged over the pairs it spans (so long lags aren't weaker).
fn autocorrelation(values: &[f64]) -> Vec<f64> {
    let size = (2 * values.len()).next_power_of_two();
    let mut planner = FftPlanner::<f64>::new();
    let mut buffer: Vec<Complex<f64>> = values
        .iter()
        .map(|value| Complex::new(*value, 0.))
        .chain(std::iter::repeat(Complex::default()))
        .take(size)
        .collect();
    planner.plan_fft_forward(size).process(&mut buffer);
    for bin in &mut buffer {
        *bin = Complex::new(bin.norm_sqr(), 0.);
    }
    planner.plan_fft_inverse(size).process(&mut buffer);
    (0..=values.len() / 2)
        .map(|lag| buffer[lag].re / size as f64 / (values.len() - lag) as f64)
        .collect()
}

/// The highest autocorrelation within a value of `lag`: a period that
/// falls between two values shows on both.
fn peak_near(correlation: &[f64], lag: usize) -> Option<f64> {
    let from = lag.saturating_sub(1).max(1);
    correlation
        .get(from..=(lag + 1).min(correlation.len().checked_sub(1)?))?
        .iter()
        .copied()
        .reduce(f64::max)
}

/// The period near `period`, to a fraction of a value: read on ever
/// larger multiples of it, where an error of a value is a smaller fraction.
fn refine(correlation: &[f64], mut period: f64) -> f64 {
    let mut multiple = 1;
    while multiple <= REFINE_UP_TO {
        let centre = (period * multiple as f64).round() as usize;
        if centre + 3 >= correlation.len() {
            break;
        }
        let from = centre.saturating_sub(2).max(1);
        let peak = (from..=centre + 2)
            .max_by(|a, b| correlation[*a].total_cmp(&correlation[*b]))
            .unwrap_or(centre);
        period = interpolate_peak(correlation, peak) / multiple as f64;
        multiple *= 2;
    }
    period
}

/// Where the parabola through `index` and its neighbours peaks.
fn interpolate_peak(values: &[f64], index: usize) -> f64 {
    let (Some(before), Some(at), Some(after)) = (
        index.checked_sub(1).and_then(|before| values.get(before)),
        values.get(index),
        values.get(index + 1),
    ) else {
        return index as f64;
    };
    let curvature = before - 2. * at + after;
    if curvature >= 0. {
        return index as f64;
    }
    index as f64 + (0.5 * (before - after) / curvature).clamp(-0.5, 0.5)
}

/// The tempos at which a loop `seconds` long holds a whole number of bars.
fn bar_tempos(seconds: f64) -> impl Iterator<Item = f64> {
    LOOP_BEATS
        .iter()
        .map(move |beats| 60. * beats / seconds)
        .filter(|bpm| (MIN_BPM..=MAX_BPM).contains(bpm))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onset::signals::*;
    use crate::sample::SampleBuffer;

    const RATE: f64 = 48000.;

    fn tempo(samples: Vec<f32>) -> Option<TempoEstimate> {
        estimate(&OnsetCurve::from_buffer(&SampleBuffer::new(
            vec![samples],
            RATE,
        )))
    }

    #[test]
    fn click_tracks_are_found_within_half_a_bpm() {
        for bpm in [85., 100., 120., 128., 140., 160.] {
            // Not a whole number of bars: read from the curve alone.
            let found = tempo(click_track(bpm, 31.3, RATE)).expect("a tempo");
            assert!(
                (found.bpm - bpm).abs() <= 0.5,
                "{bpm} BPM found as {}",
                found.bpm
            );
            assert!(found.confidence > 0.5, "{bpm} BPM: {}", found.confidence);
        }
    }

    /// Loops of whole bars, beats only, are found at their tempo: not
    /// halved or doubled.
    #[test]
    fn loops_are_found_at_their_tempo() {
        for bpm in [90., 128.] {
            for bars in [1., 2., 4.] {
                let seconds = bars * 4. * 60. / bpm;
                let found = tempo(click_track(bpm, seconds, RATE)).expect("a tempo");
                assert!(
                    (found.bpm - bpm).abs() <= 0.05,
                    "{bars} bars at {bpm} BPM found as {}",
                    found.bpm
                );
            }
        }
    }

    /// A beat with off-beat hits between is still found at its tempo, not
    /// at twice it.
    #[test]
    fn off_beats_dont_double_it() {
        let bpm = 100.;
        let beat = 60. / bpm;
        let seconds = 16. * beat;
        let times = (0..16).flat_map(|index| {
            let at = index as f64 * beat;
            [at, at + beat / 2.]
        });
        let mut samples = hits(times.clone().step_by(2), seconds, RATE);
        // Off-beats softer.
        for (sample, off) in samples
            .iter_mut()
            .zip(hits(times.skip(1).step_by(2), seconds, RATE))
        {
            *sample += off * 0.4;
        }
        let found = tempo(samples).expect("a tempo");
        assert!((found.bpm - bpm).abs() <= 0.05, "found {}", found.bpm);
    }

    #[test]
    fn no_tempo_without_a_steady_beat() {
        let seconds = 8.;
        let len = (seconds * RATE) as usize;
        assert_eq!(tempo(vec![0.; len]), None, "silence");
        assert_eq!(tempo(hits([0.5], seconds, RATE)), None, "a single hit");
        for (seconds, seed) in [(1.6, 1), (2.5, 2), (4., 3), (8., 4)] {
            let len = (seconds * RATE) as usize;
            let noise = noise(len, seed).iter().map(|sample| sample * 0.5).collect();
            assert_eq!(tempo(noise), None, "{seconds} s of noise");
        }
        assert_eq!(tempo(click_track(120., 1., RATE)), None, "a one-shot");
    }

    #[test]
    fn bar_tempos_are_whole_bars_in_range() {
        // 2 bars at 128 BPM.
        let seconds = 8. * 60. / 128.;
        let tempos: Vec<f64> = bar_tempos(seconds).collect();
        assert_eq!(tempos, [64., 128.]);
    }
}
