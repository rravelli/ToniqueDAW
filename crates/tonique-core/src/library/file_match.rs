//! Finding the audio files a project uses that weren't found: the audio
//! files of the browser's folders, read in the background, and those like
//! a missing one, best first.

use std::{
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, channel},
};

use tonique_engine::edit::SourceId;

use crate::library::file_tree::is_audio_file;

/// Extensions offered when picking a file on disk; any readable file is
/// taken.
pub use crate::library::waveform::DECODED_EXTENSIONS as AUDIO_EXTENSIONS;
/// Most matches listed for a file.
const MAX_MATCHES: usize = 40;
/// Most files read from the browser's folders: enough for a sample
/// library, not a whole disk.
const MAX_INDEXED: usize = 300_000;

pub fn file_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
}

/// The name of the folder `path` is in.
pub fn folder_name(path: &Path) -> &str {
    path.parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or_default()
}

/// The folder `path` is in, in full.
pub fn folder_of(path: &Path) -> String {
    path.parent().unwrap_or(path).display().to_string()
}

/// What a missing file is searched for by: its name, without extension.
pub fn query_for(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Lowercase words and numbers of `text`: `FL_140BPM Kit-5` gives `fl`,
/// `140`, `bpm`, `kit`, `5`.
fn words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut digits = false;
    for character in text.chars() {
        if !character.is_alphanumeric() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            continue;
        }
        if !word.is_empty() && character.is_ascii_digit() != digits {
            words.push(std::mem::take(&mut word));
        }
        digits = character.is_ascii_digit();
        word.extend(character.to_lowercase());
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Files of `files` like `missing`, as searched for by `query`, best
/// first: its very name, then names with every word of the query, then
/// names with most of them; in a folder named as `missing`'s first among
/// equals.
pub fn rank(missing: &Path, query: &str, files: &[PathBuf]) -> Vec<PathBuf> {
    let wanted = words(query);
    if wanted.is_empty() {
        return Vec::new();
    }
    let name = file_name(missing);
    let folder = folder_name(missing);
    // Searching by the file's own name, names with most of its words are
    // worth a look; what's typed is meant whole.
    let loose = query == query_for(missing);
    let mut scored: Vec<(u32, &PathBuf)> = files
        .iter()
        .filter_map(|found| {
            let have = words(&query_for(found));
            let matched = wanted
                .iter()
                .filter(|word| have.iter().any(|had| had.starts_with(word.as_str())))
                .count();
            let mut score = if matched == wanted.len() {
                if file_name(found).eq_ignore_ascii_case(name) {
                    1000
                } else {
                    // Fewer extra words: closer.
                    600 - (have.len().saturating_sub(matched) as u32).min(100)
                }
            } else if loose && 2 * matched >= wanted.len() && matched > 0 {
                (300 * matched / wanted.len()) as u32
            } else {
                return None;
            };
            if !folder.is_empty() && folder_name(found).eq_ignore_ascii_case(folder) {
                score += 50;
            }
            Some((score, found))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    scored
        .into_iter()
        .take(MAX_MATCHES)
        .map(|(_, found)| found.clone())
        .collect()
}

/// For each missing file (its source, name and path), the file of the
/// same name in `files`, if there's one: the one in a folder of the same
/// name when there are several.
pub fn exact_matches(
    missing: &[(SourceId, String, PathBuf)],
    files: &[PathBuf],
) -> Vec<(SourceId, PathBuf)> {
    missing
        .iter()
        .filter_map(|(id, name, path)| {
            let same: Vec<&PathBuf> = files
                .iter()
                .filter(|found| file_name(found).eq_ignore_ascii_case(name))
                .collect();
            let found = match same.as_slice() {
                [one] => *one,
                many => many
                    .iter()
                    .find(|found| folder_name(found) == folder_name(path))?,
            };
            Some((*id, found.clone()))
        })
        .collect()
}

/// The audio files in the browser's folders, read in the background.
#[derive(Default)]
pub struct FileIndex {
    /// The folders read, or being read.
    folders: Vec<PathBuf>,
    files: Vec<PathBuf>,
    scanning: Option<Receiver<Vec<PathBuf>>>,
}

impl FileIndex {
    /// Read `folders` again if they changed; take the files once read.
    pub fn update(&mut self, folders: &[PathBuf]) {
        if self.folders != folders {
            self.folders = folders.to_vec();
            let (sender, receiver) = channel();
            let folders = self.folders.clone();
            std::thread::spawn(move || {
                let _ = sender.send(audio_files(&folders));
            });
            self.scanning = Some(receiver);
        }
        if let Some(files) = self
            .scanning
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.files = files;
            self.scanning = None;
        }
    }

    pub fn is_scanning(&self) -> bool {
        self.scanning.is_some()
    }

    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }
}

/// The audio files in `folders` and their subfolders, hidden ones aside.
fn audio_files(folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = folders.to_vec();
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => pending.push(path),
                Ok(_) if is_audio_file(&path) => {
                    files.push(path);
                    if files.len() >= MAX_INDEXED {
                        return files;
                    }
                }
                _ => {}
            }
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_split_into_words_and_numbers() {
        assert_eq!(
            words("FL_140BPM Kit-5 éte"),
            ["fl", "140", "bpm", "kit", "5", "éte"]
        );
    }

    /// The very name first, then names with every word, closest first;
    /// in a folder named alike first among equals.
    #[test]
    fn matches_are_ranked_by_name() {
        let files: Vec<PathBuf> = [
            "/lib/other/kick_soft.wav",
            "/lib/drums/SH_Kick.wav",
            "/lib/kicks/SH_Kick_02.wav",
            "/lib/One Shots/SH_Kick_02.wav",
            "/lib/snare.wav",
        ]
        .map(PathBuf::from)
        .into();
        let missing = Path::new("/gone/One Shots/SH_Kick_02.wav");
        assert_eq!(
            rank(missing, &query_for(missing), &files),
            [
                Path::new("/lib/One Shots/SH_Kick_02.wav"),
                Path::new("/lib/kicks/SH_Kick_02.wav"),
                Path::new("/lib/drums/SH_Kick.wav"),
            ]
        );
        // Typed: every word, as the start of one.
        assert_eq!(
            rank(missing, "kick sof", &files),
            [Path::new("/lib/other/kick_soft.wav")]
        );
        assert!(rank(missing, "", &files).is_empty());
    }
}
