//! Colours as the project keeps them (UI toolkit aside), and as text, as
//! saved in theme and project files.

/// An sRGB colour, its alpha not premultiplied: a track's, or one read from
/// a file. The UI converts it to its toolkit's colour type where it draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Tracks before the UI gives them a palette.
    pub const GRAY: Color = Color::rgb(160, 160, 160);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

/// `#rrggbb` or `#rrggbbaa` (alpha not premultiplied).
pub fn parse_color(text: &str) -> Option<Color> {
    let hex = text.strip_prefix('#')?;
    if !matches!(hex.len(), 6 | 8) || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    let alpha = if hex.len() == 8 { byte(6)? } else { 255 };
    Some(Color::rgba(byte(0)?, byte(2)?, byte(4)?, alpha))
}

/// Inverse of [`parse_color`]: `#rrggbb` when opaque.
pub fn format_color(color: Color) -> String {
    let Color { r, g, b, a } = color;
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
        assert_eq!(parse_color("#00c896"), Some(Color::rgb(0, 200, 150)));
        let faint_white = Color::rgba(255, 255, 255, 14);
        assert_eq!(parse_color("#ffffff0e"), Some(faint_white));
        assert_eq!(format_color(faint_white), "#ffffff0e");
        for bad in ["00c896", "#00c89", "#00c8961", "#gg0000", "#éé0000"] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
    }
}
