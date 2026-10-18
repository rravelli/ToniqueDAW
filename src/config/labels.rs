//! Colour labels on files and folders of the browser: a few colours, each
//! with a name of its own, kept with the browser's settings (not in
//! projects: they're about a library, not one song).

use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::Path;
use std::{collections::BTreeMap, path::PathBuf};

/// What labels can be recoloured to, `#rrggbb`.
pub const COLORS: [&str; 10] = [
    "#e5604c", "#ec8a3f", "#e8b33c", "#8cc152", "#4fbf7f", "#3fb8c8", "#4a9fe0", "#9a7be6",
    "#d46cc8", "#9aa0a8",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Labels {
    labels: Vec<Label>,
    /// Each labelled file or folder, and its label's index.
    #[serde(default)]
    items: BTreeMap<PathBuf, usize>,
}

impl Default for Labels {
    /// Seven labels, named by their colours.
    fn default() -> Self {
        let label = |name: &str, color: &str| Label {
            name: name.into(),
            color: color.into(),
        };
        Self {
            labels: vec![
                label("Red", COLORS[0]),
                label("Orange", COLORS[1]),
                label("Yellow", COLORS[2]),
                label("Green", COLORS[4]),
                label("Blue", COLORS[6]),
                label("Purple", COLORS[7]),
                label("Grey", COLORS[9]),
            ],
            items: BTreeMap::new(),
        }
    }
}

impl Labels {
    pub fn all(&self) -> &[Label] {
        &self.labels
    }

    /// The label of `path` itself (not of a folder it's in).
    #[cfg(test)]
    pub fn of(&self, path: &Path) -> Option<usize> {
        self.items.get(path).copied()
    }

    /// Every labelled file or folder, and its label.
    pub fn assigned(&self) -> impl Iterator<Item = (&PathBuf, usize)> {
        self.items.iter().map(|(path, label)| (path, *label))
    }

    /// Everything with label `index` that's still there, by path.
    pub fn items(&self, index: usize) -> Vec<PathBuf> {
        self.items
            .iter()
            .filter(|(path, label)| **label == index && path.exists())
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Label `path` with `label`, or take its label off (`None`).
    pub fn set(&mut self, path: PathBuf, label: Option<usize>) {
        match label.filter(|&l| l < self.labels.len()) {
            Some(label) => {
                self.items.insert(path, label);
            }
            None => {
                self.items.remove(&path);
            }
        }
    }

    pub fn rename(&mut self, index: usize, name: String) {
        if let Some(label) = self.labels.get_mut(index) {
            label.name = name;
        }
    }

    pub fn recolor(&mut self, index: usize, color: String) {
        if let Some(label) = self.labels.get_mut(index) {
            label.color = color;
        }
    }

    /// Take label `index` off everything.
    pub fn clear(&mut self, index: usize) {
        self.items.retain(|_, label| *label != index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_are_labelled_relabelled_and_cleared() {
        let mut labels = Labels::default();
        let dir = std::env::temp_dir();
        let (a, gone) = (dir.clone(), dir.join("tonique-no-such-file.wav"));
        labels.set(a.clone(), Some(1));
        labels.set(gone.clone(), Some(1));
        assert_eq!(labels.of(&a), Some(1));
        assert_eq!(
            labels.items(1),
            std::slice::from_ref(&a),
            "files gone are left out"
        );
        labels.set(a.clone(), Some(2));
        assert_eq!(labels.items(1), [] as [PathBuf; 0], "one label each");
        labels.set(a.clone(), Some(99));
        assert_eq!(labels.of(&a), None, "no such label: none");
        labels.set(a.clone(), Some(2));
        labels.clear(2);
        assert_eq!(labels.of(&a), None);
    }
}
