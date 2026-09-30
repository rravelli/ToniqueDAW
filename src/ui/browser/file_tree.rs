use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::read_dir,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct FolderNode {
    pub open: bool,
    pub path: PathBuf,
    children: Option<Vec<FileNode>>,
    pub search_result: Vec<FileNode>,
    pub depth: usize,
}

impl FolderNode {
    pub fn new(path: &Path, depth: usize) -> Self {
        Self {
            open: false,
            path: path.to_path_buf(),
            children: None,
            search_result: Vec::new(),
            depth,
        }
    }

    pub fn opened(mut self) -> Self {
        self.open = true;
        self
    }

    /// What's inside, read from disk the first time.
    pub fn children(&mut self) -> Vec<FileNode> {
        if let Some(children) = self.children.clone() {
            children
        } else {
            let children = read_entries(self.path.clone(), self.depth + 1);
            self.children = Some(children.clone());
            children
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileNode {
    pub path: PathBuf,
    pub depth: usize,
    name_lower: String,
    pub is_dir: bool,
}

pub struct FileTree {
    pub folders: HashMap<PathBuf, FolderNode>,
    pub items: Vec<FileNode>,
    pub query: String,
}

impl FileTree {
    pub fn new() -> Self {
        Self {
            folders: HashMap::new(),
            items: Vec::new(),
            query: "".to_string(),
        }
    }

    pub fn init(&mut self, dir: PathBuf) {
        self.folders.clear();
        self.folders
            .insert(dir.clone(), FolderNode::new(&dir, 0).opened());

        let children = self.visible_under(&FileNode {
            path: dir,
            depth: 0,
            name_lower: String::new(),
            is_dir: true,
        });

        self.items = children;
    }

    pub fn rebuild(&mut self, root: PathBuf) {
        let children = self.visible_under(&FileNode {
            path: root,
            depth: 0,
            name_lower: String::new(),
            is_dir: true,
        });
        self.items = children;
    }

    /// Everything shown under `dir`: its children, and theirs in open
    /// folders.
    fn visible_under(&mut self, dir: &FileNode) -> Vec<FileNode> {
        let mut stack = Vec::<FileNode>::new();
        let mut children = Vec::new();

        stack.push(dir.clone());

        while let Some(file) = stack.pop() {
            children.push(file.clone());

            if !file.is_dir {
                continue;
            }

            // children.push(file);
            self.folders
                .entry(file.path.clone())
                .or_insert_with(|| FolderNode::new(&file.path, file.depth));

            if let Some(dir) = self.folders.get_mut(&file.path)
                && dir.open
            {
                let dir_children = if self.query.is_empty() {
                    dir.children()
                } else {
                    dir.search_result.clone()
                };
                stack.extend(dir_children);
            }
        }

        children.remove(0);

        children
    }

    pub fn toggle_folder(&mut self, index: usize) {
        let Some(file) = self.items.get(index).cloned() else {
            return;
        };
        if !file.is_dir {
            return;
        }

        let is_open = self
            .folders
            .get(&file.path)
            .is_some_and(|folder| folder.open);
        if is_open {
            if let Some(folder) = self.folders.get_mut(&file.path) {
                folder.open = false;
            }
            let end = (index + 1..self.items.len())
                .find(|&i| self.items[i].depth <= file.depth)
                .unwrap_or(self.items.len());
            self.items.drain(index + 1..end);
        } else {
            if let Some(folder) = self.folders.get_mut(&file.path) {
                folder.open = true;
            }
            let children = self.visible_under(&file);
            self.items.splice(index + 1..index + 1, children);
        }
    }

    pub fn search(&mut self, root: PathBuf, query: &str) {
        filter(root, &mut self.folders, &query.to_lowercase(), 0, false);
    }
}

pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "mp3" | "wav" | "flac" | "ogg" | "aiff" | "aac" | "m4a" | "midi" | "mid"
            )
        })
        .unwrap_or(false)
}

fn read_entries(dir: PathBuf, depth: usize) -> Vec<FileNode> {
    let mut entries = read_dir(dir)
        .map(|read_dir| {
            read_dir
                .filter_map(|entry| entry.ok())
                .map(|f| f.path())
                .filter(|path| path.is_dir() || is_audio_file(path))
                .map(|entry| {
                    let name_lower = entry
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let is_dir = entry.is_dir();
                    FileNode {
                        depth,
                        path: entry,
                        name_lower,
                        is_dir,
                    }
                })
                .collect()
        })
        .unwrap_or_else(|_| vec![]);

    entries.sort_by_key(|file| {
        file.path
            .file_name()
            .map(|s| s.to_os_string())
            .unwrap_or_default()
    });

    entries.reverse();

    entries
}

fn filter(
    root: PathBuf,
    folders: &mut HashMap<PathBuf, FolderNode>,
    query: &str,
    depth: usize,
    include_all: bool,
) -> bool {
    let children = if let Some(folder) = folders.get_mut(&root) {
        folder.children()
    } else {
        let mut node = FolderNode::new(&root, depth);
        let c = node.children();
        folders.insert(root.clone(), node);
        c
    };

    let mut results = Vec::new();
    let mut contains_valid = false;
    for child in children {
        let is_valid = child.name_lower.contains(query) || include_all;

        if !child.is_dir {
            if is_valid {
                results.push(child);
                contains_valid = true;
            }
        } else {
            let child_valid = filter(child.path.clone(), folders, query, depth + 1, is_valid);
            if child_valid || is_valid {
                results.push(child);
                contains_valid = true;
            }
        }
    }
    if let Some(folder) = folders.get_mut(&root) {
        folder.search_result = results;
    }

    contains_valid
}
