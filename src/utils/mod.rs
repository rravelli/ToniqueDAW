use tonique_engine::time::{BeatPos, TimeSignature};

pub mod color;
#[cfg(test)]
mod tests;

/// A position in beats as `bar.beat.sixteenth`, counting from 1, beats as
/// `signature` counts them: beat 0 is `1.1.1`. Before beat 0 (as in a view counted from a clip's start), bars
/// count down from -1, the bar right before bar 1.
pub fn format_bars(beats: f64, signature: TimeSignature) -> String {
    let at = signature.locate(BeatPos(beats + 1e-6));
    let bar = if at.bar >= 0 { at.bar + 1 } else { at.bar };
    let sixteenth = (at.offset * 4.).floor() as u64 + 1;
    format!("{bar}.{}.{sixteenth}", at.beat + 1)
}

/// `m:ss.mmm`, with a `-` before zero.
pub fn format_time(seconds: f64) -> String {
    let millis = (seconds * 1000.).round() as i64;
    let sign = if millis < 0 { "-" } else { "" };
    let millis = millis.unsigned_abs();
    format!(
        "{sign}{}:{:02}.{:03}",
        millis / 60_000,
        millis / 1000 % 60,
        millis % 1000
    )
}

/// Pan closer to the centre than this snaps to it while dragging.
const PAN_SNAP: f32 = 0.02;

/// A pan (-1 left to 1 right): `C` for centre, else `L`/`R` and a
/// percentage, `L35`.
pub fn format_pan(pan: f32) -> String {
    let percent = (pan.abs() * 100.).round();
    match percent as u32 {
        0 => "C".into(),
        n if pan < 0. => format!("L{n}"),
        n => format!("R{n}"),
    }
}

/// Close enough to the centre to be meant as the centre.
pub fn snap_to_centre(pan: f32) -> f32 {
    if pan.abs() < PAN_SNAP { 0. } else { pan }
}

pub fn display_name(name: &str, index: usize) -> String {
    name.replace("#", &format!("{}", index + 1))
}
