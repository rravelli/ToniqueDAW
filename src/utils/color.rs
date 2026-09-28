//! Colours as text, as saved in theme and project files.

// Reading colours from files builds them from values (see `clippy.toml`).
#![allow(clippy::disallowed_methods)]

use egui::Color32;

/// `#rrggbb` or `#rrggbbaa` (alpha not premultiplied).
pub fn parse_color(text: &str) -> Option<Color32> {
    let hex = text.strip_prefix('#')?;
    if !matches!(hex.len(), 6 | 8) || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    let alpha = if hex.len() == 8 { byte(6)? } else { 255 };
    Some(Color32::from_rgba_unmultiplied(
        byte(0)?,
        byte(2)?,
        byte(4)?,
        alpha,
    ))
}

/// Inverse of [`parse_color`]: `#rrggbb` when opaque.
pub fn format_color(color: Color32) -> String {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_round_trip_through_text() {
        assert_eq!(parse_color("#00c896"), Some(Color32::from_rgb(0, 200, 150)));
        assert_eq!(
            parse_color("#ffffff0e"),
            Some(Color32::from_white_alpha(14))
        );
        assert_eq!(format_color(Color32::from_white_alpha(14)), "#ffffff0e");
        for bad in ["00c896", "#00c89", "#00c8961", "#gg0000", "#éé0000"] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
    }
}
