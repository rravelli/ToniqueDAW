//! Project files: the arrangement, mixer, effects and how tracks look, as
//! JSON. Audio files are referenced in place, relative to the project when
//! they're inside its folder, so moving the whole folder keeps working.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::ui::effects::EffectId;

/// Extension of project files.
pub const EXTENSION: &str = "tonique";
/// Format version written; files from newer versions are refused.
/// 2 added groups.
pub const VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectFile {
    pub version: u32,
    pub bpm: f32,
    /// Loop region in beats.
    pub loop_range: (f32, f32),
    pub looping: bool,
    pub master: ChannelFile,
    /// Parents before their subgroups.
    #[serde(default)]
    pub groups: Vec<GroupFile>,
    /// In display order.
    pub tracks: Vec<TrackFile>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupFile {
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
    pub height: f32,
    /// Older projects call it `folded`.
    #[serde(alias = "folded")]
    pub collapsed: bool,
    pub soloed: bool,
    /// Index in `groups` of the group holding it.
    pub parent: Option<usize>,
    #[serde(flatten)]
    pub channel: ChannelFile,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackFile {
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
    pub height: f32,
    /// Older projects call it `closed`.
    #[serde(alias = "closed")]
    pub collapsed: bool,
    pub soloed: bool,
    /// Index in `groups` of the group holding it.
    #[serde(default)]
    pub group: Option<usize>,
    #[serde(flatten)]
    pub channel: ChannelFile,
    pub clips: Vec<ClipFile>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelFile {
    /// Linear gain.
    pub volume: f32,
    /// -1 (left) to 1 (right).
    pub pan: f32,
    pub muted: bool,
    /// In processing order.
    #[serde(default)]
    pub effects: Vec<EffectFile>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClipFile {
    /// The audio file: relative to the project's folder, or absolute.
    pub path: PathBuf,
    /// Start in the arrangement, in beats.
    pub position: f32,
    /// Part of the file played, as ratios of its length.
    pub trim_start: f32,
    pub trim_end: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectFile {
    pub kind: EffectId,
    pub enabled: bool,
    /// Parameter values by name.
    pub params: BTreeMap<String, f32>,
}

impl ProjectFile {
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let project: Self = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if project.version > VERSION {
            return Err(format!(
                "made by a newer version of Tonique (format {}, this one reads up to {VERSION})",
                project.version
            ));
        }
        Ok(project)
    }

    /// Write to a temporary file first, so a failed save never leaves a
    /// half-written project behind.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let temporary = path.with_extension(format!("{EXTENSION}.saving"));
        fs::write(&temporary, text).map_err(|e| e.to_string())?;
        fs::rename(&temporary, path).map_err(|e| e.to_string())
    }
}

/// How an audio file is stored in a project in `dir`: relative when it's
/// inside the folder, absolute otherwise.
pub fn store_path(path: &Path, dir: Option<&Path>) -> PathBuf {
    dir.and_then(|dir| path.strip_prefix(dir).ok())
        .map_or_else(|| path.to_path_buf(), Path::to_path_buf)
}

/// Where a stored audio file is now: as stored (relative to the project's
/// folder), or else next to the project, where a moved set of files often
/// ends up.
pub fn resolve_path(stored: &Path, dir: &Path) -> PathBuf {
    let path = dir.join(stored);
    if path.exists() {
        return path;
    }
    stored
        .file_name()
        .map(|name| dir.join(name))
        .filter(|beside| beside.exists())
        .unwrap_or(path)
}

/// The project's name: its file name without the extension.
pub fn project_name(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| "Untitled".into(), |s| s.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tonique-project-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn paths_are_relative_inside_the_project_folder() {
        let dir = Path::new("/music/song");
        assert_eq!(
            store_path(Path::new("/music/song/drums/kick.wav"), Some(dir)),
            Path::new("drums/kick.wav")
        );
        assert_eq!(
            store_path(Path::new("/samples/kick.wav"), Some(dir)),
            Path::new("/samples/kick.wav")
        );
        assert_eq!(
            store_path(Path::new("/samples/kick.wav"), None),
            Path::new("/samples/kick.wav")
        );
    }

    #[test]
    fn missing_files_are_looked_for_next_to_the_project() {
        let dir = temp_dir("resolve");
        fs::write(dir.join("kick.wav"), b"").unwrap();
        assert_eq!(
            resolve_path(Path::new("kick.wav"), &dir),
            dir.join("kick.wav")
        );
        // Moved from elsewhere, now next to the project.
        assert_eq!(
            resolve_path(Path::new("/gone/kick.wav"), &dir),
            dir.join("kick.wav")
        );
        // Nowhere: reported as stored.
        assert_eq!(
            resolve_path(Path::new("/gone/snare.wav"), &dir),
            Path::new("/gone/snare.wav")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_round_trip_and_newer_versions_are_refused() {
        let dir = temp_dir("file");
        let path = dir.join("song.tonique");
        let mut project = ProjectFile {
            version: VERSION,
            bpm: 128.,
            loop_range: (4., 12.),
            looping: true,
            master: ChannelFile {
                volume: 0.8,
                pan: 0.,
                muted: false,
                effects: Vec::new(),
            },
            groups: Vec::new(),
            tracks: vec![TrackFile {
                name: "Drums".into(),
                color: "#e5736b".into(),
                height: 60.,
                collapsed: false,
                soloed: true,
                group: None,
                channel: ChannelFile {
                    volume: 0.5,
                    pan: -0.25,
                    muted: true,
                    effects: vec![EffectFile {
                        kind: EffectId::Equalizer,
                        enabled: false,
                        params: BTreeMap::from([("cutoff".into(), 800.)]),
                    }],
                },
                clips: vec![ClipFile {
                    path: "kick.wav".into(),
                    position: 2.,
                    trim_start: 0.1,
                    trim_end: 0.9,
                }],
            }],
        };
        project.write(&path).unwrap();
        assert_eq!(ProjectFile::read(&path).unwrap(), project);
        assert!(!dir.join("song.tonique.saving").exists());

        project.version = VERSION + 1;
        project.write(&path).unwrap();
        assert!(
            ProjectFile::read(&path)
                .unwrap_err()
                .contains("newer version")
        );
        assert_eq!(project_name(&path), "song");
        let _ = fs::remove_dir_all(&dir);
    }
}
