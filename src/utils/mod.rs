pub mod color;
#[cfg(test)]
mod tests;

/// A position in beats as `bar.beat.sixteenth`, counting from 1: beat 0
/// is `1.1.1`. Before beat 0 (as in a view counted from a clip's start), bars
/// count down from -1, the bar right before bar 1.
pub fn format_bars(beats: f64, beats_per_bar: usize) -> String {
    let sixteenths = (beats * 4. + 1e-6).floor() as i64;
    let per_bar = beats_per_bar.max(1) as i64 * 4;
    let bar = sixteenths.div_euclid(per_bar);
    let bar = if bar >= 0 { bar + 1 } else { bar };
    let within = sixteenths.rem_euclid(per_bar);
    format!("{bar}.{}.{}", within / 4 + 1, within % 4 + 1)
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

pub fn display_name(name: &str, index: usize) -> String {
    name.replace("#", &format!("{}", index + 1))
}
