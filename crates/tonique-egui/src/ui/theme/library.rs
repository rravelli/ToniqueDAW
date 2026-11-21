//! The themes to choose from: built-in presets and `*.toml` files in the
//! user's themes folder. A theme is built from Dark, then each theme it
//! `extends`, then its own colours.

use std::{fs, path::PathBuf};

use super::{
    Theme,
    file::{ThemeSource, to_toml},
};
use crate::config::config_dir;

const BASE: &str = "dark";
/// Dark first: it's the base of every theme.
const PRESETS: [(&str, &str); 9] = [
    ("dark", include_str!("presets/dark.toml")),
    ("light", include_str!("presets/light.toml")),
    ("session", include_str!("presets/session.toml")),
    ("slate", include_str!("presets/slate.toml")),
    ("studio", include_str!("presets/studio.toml")),
    ("graphite", include_str!("presets/graphite.toml")),
    ("console", include_str!("presets/console.toml")),
    ("tape", include_str!("presets/tape.toml")),
    ("high-contrast", include_str!("presets/high-contrast.toml")),
];
/// Longest `extends` chain followed, to stop on cycles.
const MAX_DEPTH: usize = 8;

/// Dark, which sets every colour and so is the start of every theme.
pub(super) fn base_theme() -> Theme {
    let mut theme = Theme::blank();
    if let Ok(source) = ThemeSource::parse(PRESETS[0].1) {
        source.apply(&mut theme);
    }
    theme
}

struct Entry {
    id: String,
    /// `None` for presets.
    path: Option<PathBuf>,
    source: Result<ThemeSource, String>,
}

/// A theme as listed in the settings.
pub struct ThemeSummary {
    /// Preset name or file name without `.toml`; what settings store.
    pub id: String,
    pub name: String,
    pub preset: bool,
}

pub struct ThemeLibrary {
    entries: Vec<Entry>,
    /// Where user themes are read from and written to.
    dir: Option<PathBuf>,
}

impl ThemeLibrary {
    /// The presets, then the themes in `<config dir>/themes`.
    pub fn load() -> Self {
        Self::load_from(config_dir().map(|dir| dir.join("themes")))
    }

    /// The presets, then the files of `dir` by file name.
    fn load_from(dir: Option<PathBuf>) -> Self {
        let mut entries: Vec<Entry> = PRESETS
            .iter()
            .map(|(id, text)| Entry {
                id: id.to_string(),
                path: None,
                source: ThemeSource::parse(text),
            })
            .collect();
        let mut files: Vec<PathBuf> = dir
            .as_ref()
            .and_then(|dir| fs::read_dir(dir).ok())
            .into_iter()
            .flatten()
            .filter_map(|entry| Some(entry.ok()?.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
            .collect();
        files.sort();
        for path in files {
            let Some(id) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
                continue;
            };
            let source = if entries.iter().any(|e| e.id == id) {
                Err(format!("“{id}” is a built-in theme: rename the file"))
            } else {
                fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|text| ThemeSource::parse(&text))
            };
            entries.push(Entry {
                id,
                path: Some(path),
                source,
            });
        }
        Self { entries, dir }
    }

    /// Where user themes live.
    pub fn dir(&self) -> Option<&PathBuf> {
        self.dir.as_ref()
    }

    pub fn is_preset(&self, id: &str) -> bool {
        self.entries.iter().any(|e| e.id == id && e.path.is_none())
    }

    /// The theme `id` builds on: what it `extends`, else Dark.
    pub fn base_of(&self, id: &str) -> String {
        let extends = self
            .entries
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| e.source.as_ref().ok())
            .and_then(|s| s.extends.clone());
        extends.unwrap_or_else(|| BASE.to_string())
    }

    /// Copy theme `from` into a new file of the themes folder, named
    /// "<name> copy". Returns the new theme's id.
    pub fn duplicate(&mut self, from: &str) -> Result<String, String> {
        let dir = self.dir.clone().ok_or("no themes folder on this system")?;
        let (mut theme, _) = self.resolve(from);
        // A copy of a preset builds on it; a copy of a custom theme builds
        // on the same theme as the original.
        let base = if self.is_preset(from) {
            from.to_string()
        } else {
            self.base_of(from)
        };

        let mut name = format!("{} copy", theme.name);
        let mut id = slug(&name);
        let mut n = 2;
        while self.entries.iter().any(|e| e.id == id) || dir.join(format!("{id}.toml")).exists() {
            name = format!("{} copy {n}", theme.name);
            id = slug(&name);
            n += 1;
        }
        theme.name = name;

        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{id}.toml"));
        self.write(&path, &theme, &base)?;
        self.entries.push(Entry {
            id: id.clone(),
            path: Some(path),
            source: fs::read_to_string(dir.join(format!("{id}.toml")))
                .map_err(|e| e.to_string())
                .and_then(|text| ThemeSource::parse(&text)),
        });
        Ok(id)
    }

    /// Write `theme` to the file of user theme `id`, keeping what it
    /// extends and listing only the colours that differ from it.
    pub fn save(&mut self, id: &str, theme: &Theme) -> Result<(), String> {
        let base = self.base_of(id);
        let entry = self.entries.iter().position(|e| e.id == id);
        let Some(path) = entry.and_then(|i| self.entries[i].path.clone()) else {
            return Err(format!("“{id}” is built in and can't be changed"));
        };
        let text = self.write(&path, theme, &base)?;
        if let Some(i) = entry {
            self.entries[i].source = ThemeSource::parse(&text);
        }
        Ok(())
    }

    fn write(&self, path: &PathBuf, theme: &Theme, base: &str) -> Result<String, String> {
        let (base_theme, _) = self.resolve(base);
        let text = to_toml(theme, Some((base, &base_theme)));
        fs::write(path, &text).map_err(|e| format!("couldn't save {}: {e}", path.display()))?;
        Ok(text)
    }

    pub fn list(&self) -> Vec<ThemeSummary> {
        self.entries
            .iter()
            .map(|entry| ThemeSummary {
                id: entry.id.clone(),
                name: entry
                    .source
                    .as_ref()
                    .ok()
                    .and_then(|s| s.name.clone())
                    .unwrap_or_else(|| entry.id.clone()),
                preset: entry.path.is_none(),
            })
            .collect()
    }

    /// The theme `id`, with what went wrong building it. Falls back to
    /// Dark (or the part of the chain that worked) rather than failing.
    pub fn resolve(&self, id: &str) -> (Theme, Vec<String>) {
        let mut warnings = Vec::new();
        let mut chain = Vec::new();
        let mut next = Some(id.to_string());
        while let Some(id) = next.take() {
            if chain.len() == MAX_DEPTH || chain.iter().any(|e: &&Entry| e.id == id) {
                warnings.push(format!("`extends` loops back to “{id}”"));
                break;
            }
            let Some(entry) = self.entries.iter().find(|e| e.id == id) else {
                warnings.push(format!("no theme named “{id}”"));
                break;
            };
            chain.push(entry);
            if let Ok(source) = &entry.source {
                next = source.extends.clone().filter(|base| base != BASE);
            }
        }

        let mut theme = Theme::dark();
        for entry in chain.iter().rev() {
            let file = entry
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map_or(entry.id.clone(), |f| f.to_string_lossy().into_owned());
            match &entry.source {
                Ok(source) => {
                    source.apply(&mut theme);
                    if source.name.is_none() {
                        theme.name.clone_from(&entry.id);
                    }
                    warnings.extend(source.warnings.iter().map(|w| format!("{file}: {w}")));
                }
                Err(error) => warnings.push(format!("{file}: {error}")),
            }
        }
        (theme, warnings)
    }
}

/// File name for a theme called `name`: `My theme!` → `my-theme`.
fn slug(name: &str) -> String {
    let slug = name
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "theme".into()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::COLORS;

    fn library(files: &[(&str, &str)]) -> ThemeLibrary {
        let mut library = ThemeLibrary {
            entries: Vec::new(),
            dir: None,
        };
        for (id, text) in PRESETS.iter().chain(files) {
            library.entries.push(Entry {
                id: id.to_string(),
                path: None,
                source: ThemeSource::parse(text),
            });
        }
        library
    }

    #[test]
    fn presets_parse_cleanly() {
        for (id, text) in PRESETS {
            let source = ThemeSource::parse(text).unwrap();
            assert!(source.warnings.is_empty(), "{id}: {:?}", source.warnings);
            assert!(library(&[]).resolve(id).1.is_empty(), "{id}");
        }
    }

    #[test]
    fn the_base_theme_sets_every_color() {
        let source = ThemeSource::parse(PRESETS[0].1).unwrap();
        for role in COLORS {
            assert!(
                source.colors.iter().any(|(key, _)| *key == role.key),
                "dark.toml is missing `{}`",
                role.key
            );
        }
        assert!(!source.palette.unwrap_or_default().is_empty());
    }

    #[test]
    fn themes_build_on_what_they_extend() {
        let library = library(&[
            (
                "ocean",
                "name = \"Ocean\"\nextends = \"light\"\naccent = \"#0000ff\"",
            ),
            ("bare", "accent = \"#ff0000\""),
        ]);
        let (ocean, warnings) = library.resolve("ocean");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(ocean.name, "Ocean");
        assert_eq!(ocean.accent, egui::Color32::BLUE);
        let light = library.resolve("light").0;
        assert_eq!(ocean.bg_panel, light.bg_panel);
        // Light doesn't set a palette: it comes from Dark.
        assert_eq!(light.palette, Theme::dark().palette);

        // Without `extends`, a theme builds on Dark and is named by its file.
        let (bare, _) = library.resolve("bare");
        assert_eq!(bare.name, "bare");
        assert_eq!(bare.bg_panel, Theme::dark().bg_panel);
    }

    #[test]
    fn broken_themes_fall_back_with_warnings() {
        let library = library(&[
            ("a", "extends = \"b\""),
            ("b", "extends = \"a\""),
            ("broken", "accent = "),
            ("orphan", "extends = \"nowhere\""),
        ]);
        for id in ["a", "broken", "orphan", "missing"] {
            let (theme, warnings) = library.resolve(id);
            assert!(!warnings.is_empty(), "{id}");
            assert_eq!(theme.bg_panel, Theme::dark().bg_panel, "{id}");
        }
    }

    /// A fresh folder for a test's theme files.
    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tonique-themes-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn duplicates_are_saved_as_differences() {
        let dir = temp_dir("duplicate");
        let mut library = ThemeLibrary::load_from(Some(dir.clone()));

        let id = library.duplicate("light").unwrap();
        assert_eq!(id, "light-copy");
        let (mut theme, warnings) = library.resolve(&id);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(theme.name, "Light copy");
        assert_eq!(theme.bg_panel, library.resolve("light").0.bg_panel);
        assert_eq!(library.base_of(&id), "light");

        // Only what changed from Light is written.
        theme.accent = egui::Color32::RED;
        library.save(&id, &theme).unwrap();
        let text = fs::read_to_string(dir.join("light-copy.toml")).unwrap();
        assert!(text.contains("extends = \"light\""), "{text}");
        assert_eq!(text.matches(" = \"#").count(), 1, "{text}");

        // Survives a reload, and a second copy gets its own name.
        let mut library = ThemeLibrary::load_from(Some(dir.clone()));
        assert_eq!(library.resolve(&id).0, theme);
        assert_eq!(library.duplicate("light").unwrap(), "light-copy-2");
        // A copy of a custom theme builds on the same base.
        let copy = library.duplicate(&id).unwrap();
        assert_eq!(library.base_of(&copy), "light");
        assert_eq!(library.resolve(&copy).0.accent, egui::Color32::RED);

        assert!(library.save("dark", &theme).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn slugs_are_file_names() {
        assert_eq!(slug("My theme!"), "my-theme");
        assert_eq!(slug("  Été  2 "), "été-2");
        assert_eq!(slug("!!!"), "theme");
    }
}
