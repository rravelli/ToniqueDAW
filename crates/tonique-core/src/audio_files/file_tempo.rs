//! Tempos written in file names: `loop_128bpm.wav`, `Drums 92.5 BPM.wav`,
//! `pad-128-bpm.flac`. They win over a detected tempo: whoever named the
//! file knew.

/// Tempos read from names, in BPM: outside, the number is something else.
const MIN_BPM: f64 = 40.;
const MAX_BPM: f64 = 300.;

/// The tempo in `name`: a number of 2 or 3 digits (and maybe decimals),
/// followed by "bpm" in any case, with at most a space, `-` or `_`
/// between. A bare number (`track_120.wav`) isn't one; a tempo glued to
/// a word (`Loop120BPM.wav`) is.
pub fn tempo_in_name(name: &str) -> Option<f64> {
    let lower = name.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    lower
        .match_indices("bpm")
        .filter_map(|(at, _)| number_before(bytes, at))
        .find(|bpm| (MIN_BPM..=MAX_BPM).contains(bpm))
}

/// The number ending just before `at` (but for one separator), if it has
/// 2 or 3 digits before its decimals.
fn number_before(bytes: &[u8], at: usize) -> Option<f64> {
    let mut end = at;
    if end > 0 && matches!(bytes[end - 1], b' ' | b'-' | b'_') {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && (bytes[start - 1].is_ascii_digit() || bytes[start - 1] == b'.') {
        start -= 1;
    }
    let number = std::str::from_utf8(&bytes[start..end]).ok()?;
    let digits = number.split('.').next()?.len();
    if !(2..=3).contains(&digits) || number.ends_with('.') {
        return None;
    }
    number.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempos_in_names() {
        for (name, bpm) in [
            ("loop_128bpm.wav", 128.),
            ("Loop 128 BPM.wav", 128.),
            ("pad-128-bpm.flac", 128.),
            ("pad_90_BPM.flac", 90.),
            ("Drums 92.5bpm.wav", 92.5),
            ("174bpm", 174.),
            ("kit 01 - 120bpm - Am.wav", 120.),
            ("Loop120BPM.wav", 120.),
        ] {
            assert_eq!(tempo_in_name(name), Some(bpm), "{name}");
        }
    }

    /// Numbers that only look like tempos aren't read as ones.
    #[test]
    fn names_without_a_tempo() {
        for name in [
            "track_120.wav",
            "120.wav",
            "bpm_mix.wav",
            "1280bpm.wav",
            "8bpm.wav",
            "v2120bpm.wav",
            "12 bpm.wav",
            "999bpm.wav",
            "120  bpm.wav",
            "120.bpm.wav",
            "v1.120bpm.wav",
            "",
        ] {
            assert_eq!(tempo_in_name(name), None, "{name}");
        }
    }
}
