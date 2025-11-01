use std::{collections::HashMap, ffi::OsStr, fs::read_dir, path::PathBuf};

#[derive(Debug, Clone)]
pub struct FolderNode {
    pub open: bool,
    pub path: PathBuf,
    children: Option<Vec<FileNode>>,
    pub search_result: Vec<FileNode>,
    pub depth: usize,
}

impl FolderNode {
    pub fn new(path: &PathBuf, depth: usize) -> Self {
        Self {
            open: false,
            path: path.clone(),
            children: None,
            search_result: Vec::new(),
            depth,
        }
    }

    pub fn opened(mut self) -> Self {
        self.open = true;
        self
    }

    pub fn get_children(&mut self, filter: bool) -> Vec<FileNode> {
        if filter {
            return self.search_result.clone();
        }

        if let Some(children) = self.children.clone() {
            return children;
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

        let children = self.get_children(&FileNode {
            path: dir,
            depth: 0,
        });

        self.items = children;
    }

    pub fn rebuild(&mut self, root: PathBuf) {
        let children = self.get_children(&FileNode {
            path: root,
            depth: 0,
        });
        self.items = children;
    }

    fn get_children(&mut self, dir: &FileNode) -> Vec<FileNode> {
        let mut stack = Vec::<FileNode>::new();
        let mut children = Vec::new();

        stack.push(dir.clone());

        while let Some(file) = stack.pop() {
            children.push(file.clone());

            if file.path.is_file() {
                continue;
            }

            // children.push(file);
            self.folders
                .entry(file.path.clone())
                .or_insert_with(|| FolderNode::new(&file.path, file.depth));

            if let Some(dir) = self.folders.get_mut(&file.path)
                && dir.open
            {
                let dir_children = dir.get_children(!self.query.is_empty());
                stack.extend(dir_children);
            }
        }

        children.remove(0);

        children
    }

    pub fn open_folder(&mut self, index: usize, new_items: &mut Vec<FileNode>) {
        let file = self.items[index].clone();

        // let file = self.items[index].clone();

        if let Some(dir) = self.folders.get_mut(&file.path) {
            dir.open = true
        }

        new_items.splice((index + 1)..(index + 1), self.get_children(&file));
    }

    pub fn close_folder(&mut self, index: usize, new_items: &mut Vec<FileNode>) {
        let file = self.items[index].clone();

        if let Some(dir) = self.folders.get_mut(&file.path) {
            dir.open = false
        } else {
            return;
        };

        let mut i = index + 1;
        while i < new_items.len() && new_items[i].depth > file.depth {
            i += 1;
        }
        if i > index + 1 {
            new_items.drain(index + 1..i);
        }
    }

    pub fn search(&mut self, root: PathBuf, query: &str) {
        filter(root, &mut self.folders, query, 0, false);
    }
}

pub fn is_audio_file(path: &PathBuf) -> bool {
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
                .map(|entry| FileNode {
                    depth: depth,
                    path: entry,
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
        folder.get_children(false)
    } else {
        let mut node = FolderNode::new(&root, depth);
        let c = node.get_children(false);
        folders.insert(root.clone(), node);
        c
    };

    let mut results = Vec::new();
    let mut contains_valid = false;
    for child in children {
        let name = child
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        let is_valid = name.contains(query) || include_all;

        if child.path.is_file() {
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
