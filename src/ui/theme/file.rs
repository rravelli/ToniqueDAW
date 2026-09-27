//! Theme files: TOML with a colour per key (`accent = "#00c896"`), an
//! optional `name`, `extends` naming the theme it builds on, and `palette`.

use std::fmt::Write;

use egui::Color32;
use toml::{Table, Value};

use super::{COLORS, Theme};

/// A parsed theme file, applied on top of the theme it extends.
#[derive(Clone, Debug, Default)]
pub struct ThemeSource {
    pub name: Option<String>,
    pub extends: Option<String>,
    pub colors: Vec<(&'static str, Color32)>,
    pub palette: Option<Vec<Color32>>,
    /// Entries that were skipped, e.g. an unknown key or a bad colour.
    pub warnings: Vec<String>,
}

impl ThemeSource {
    /// Fails only when the file isn't valid TOML; bad entries are skipped
    /// with a warning so the rest of the theme still applies.
    pub fn parse(text: &str) -> Result<Self, String> {
        let table: Table = text
            .parse()
            .map_err(|e: toml::de::Error| e.message().to_string())?;
        let mut source = Self::default();
        for (key, value) in table {
            match (key.as_str(), value) {
                ("name", Value::String(name)) => source.name = Some(name),
                ("extends", Value::String(base)) => source.extends = Some(base),
                ("palette", Value::Array(values)) => {
                    let palette = values
                        .iter()
                        .filter_map(|value| match value.as_str().and_then(parse_color) {
                            Some(color) => Some(color),
                            None => {
                                source.warnings.push(format!("palette: bad colour {value}"));
                                None
                            }
                        })
                        .collect();
                    source.palette = Some(palette);
                }
                (key, value) => match COLORS.iter().find(|role| role.key == key) {
                    Some(role) => match value.as_str().and_then(parse_color) {
                        Some(color) => source.colors.push((role.key, color)),
                        None => source
                            .warnings
                            .push(format!("{key}: expected \"#rrggbb\", got {value}")),
                    },
                    None => source.warnings.push(format!("unknown key `{key}`")),
                },
            }
        }
        Ok(source)
    }

    pub fn apply(&self, theme: &mut Theme) {
        if let Some(name) = &self.name {
            theme.name.clone_from(name);
        }
        for (key, color) in &self.colors {
            if let Some(slot) = theme.color_mut(key) {
                *slot = *color;
            }
        }
        if let Some(palette) = &self.palette {
            theme.palette.clone_from(palette);
        }
    }
}

/// The file for `theme`. With a base, it `extends` it and only lists what
/// differs from it.
pub fn to_toml(theme: &Theme, base: Option<(&str, &Theme)>) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "name = {}", Value::String(theme.name.clone()));
    if let Some((id, _)) = base {
        let _ = writeln!(out, "extends = {}", Value::String(id.to_string()));
    }
    let mut group = "";
    for role in COLORS {
        let color = theme.color(role.key).unwrap_or_default();
        if base.is_some_and(|(_, base)| base.color(role.key) == Some(color)) {
            continue;
        }
        if role.group != group {
            group = role.group;
            let _ = writeln!(out, "\n# {group}");
        }
        let _ = writeln!(out, "{} = \"{}\"", role.key, format_color(color));
    }
    if base.is_none_or(|(_, base)| base.palette != theme.palette) {
        out.push_str("\npalette = [\n");
        for color in &theme.palette {
            let _ = writeln!(out, "    \"{}\",", format_color(*color));
        }
        out.push_str("]\n");
    }
    out
}

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

    #[test]
    fn bad_entries_are_skipped_with_warnings() {
        let source = ThemeSource::parse(
            r##"
            name = "Mine"
            accent = "#ff0000"
            text = "red"
            sparkle = "#ffffff"
            palette = ["#00ff00", 3]
            "##,
        )
        .unwrap();
        assert_eq!(source.name.as_deref(), Some("Mine"));
        assert_eq!(source.colors, vec![("accent", Color32::RED)]);
        assert_eq!(source.palette, Some(vec![Color32::GREEN]));
        assert_eq!(source.warnings.len(), 3, "{:?}", source.warnings);
        assert!(ThemeSource::parse("accent = ").is_err());
    }

    #[test]
    fn a_theme_round_trips_through_a_file() {
        let mut theme = Theme::dark();
        theme.name = "Round \"trip\"".into();
        theme.accent = Color32::from_rgb(1, 2, 3);
        theme.palette.truncate(3);

        let mut parsed = Theme::blank();
        let source = ThemeSource::parse(&to_toml(&theme, None)).unwrap();
        assert!(source.warnings.is_empty(), "{:?}", source.warnings);
        source.apply(&mut parsed);
        assert_eq!(parsed, theme);

        // Against a base, only the differences are written.
        let text = to_toml(&theme, Some(("dark", &Theme::dark())));
        assert!(text.contains("extends = \"dark\""));
        assert_eq!(text.matches(" = \"#").count(), 1, "{text}");
        let mut parsed = Theme::dark();
        ThemeSource::parse(&text).unwrap().apply(&mut parsed);
        assert_eq!(parsed, theme);
    }
}
