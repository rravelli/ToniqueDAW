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

    /// A section of `paths` gathered from anywhere (a label's), not read
    /// from disk: `path` only names it.
    pub fn gathered(path: &Path, paths: &[PathBuf]) -> Self {
        let mut children: Vec<FileNode> = paths.iter().map(|p| FileNode::new(p, 1)).collect();
        // By name; reversed, as the rows are taken from the end.
        children.sort_by_key(|f| std::cmp::Reverse(f.name_lower.clone()));
        Self {
            children: Some(children),
            ..Self::new(path, 0)
        }
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

impl FileNode {
    fn new(path: &Path, depth: usize) -> Self {
        Self {
            name_lower: path
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default(),
            path: path.to_path_buf(),
            depth,
            is_dir: path.is_dir(),
        }
    }
}

/// The browser's folders, each a root at depth 0 with what's inside it
/// below, as shown: open folders' contents, or only what matches a search.
pub struct FileTree {
    pub folders: HashMap<PathBuf, FolderNode>,
    /// The rows shown, top to bottom.
    pub items: Vec<FileNode>,
    pub query: String,
    /// In the browser's order.
    roots: Vec<PathBuf>,
    /// Whether each root was open before a search opened them all, to
    /// close them again once it's cleared.
    open_before_search: Option<Vec<bool>>,
}

impl FileTree {
    pub fn new() -> Self {
        Self {
            folders: HashMap::new(),
            items: Vec::new(),
            query: "".to_string(),
            roots: Vec::new(),
            open_before_search: None,
        }
    }

    /// Show `roots`, open but for those in `collapsed`. Folders already
    /// read keep their contents and whether they're open.
    pub fn set_roots(&mut self, roots: Vec<PathBuf>, collapsed: &[PathBuf]) {
        for root in &roots {
            let folder = self
                .folders
                .entry(root.clone())
                .or_insert_with(|| FolderNode::new(root, 0));
            folder.open = !collapsed.contains(root);
        }
        self.roots = roots;
        self.open_before_search = None;
        self.rebuild();
    }

    /// Show `sections`, each `paths` gathered under a header named by its
    /// path, open but for those in `collapsed`.
    pub fn set_sections(&mut self, sections: Vec<(PathBuf, Vec<PathBuf>)>, collapsed: &[PathBuf]) {
        self.roots = sections.iter().map(|(header, _)| header.clone()).collect();
        for (header, paths) in sections {
            let mut folder = FolderNode::gathered(&header, &paths);
            folder.open = !collapsed.contains(&header);
            self.folders.insert(header, folder);
        }
        self.open_before_search = None;
        if self.query.is_empty() {
            self.rebuild();
        }
    }

    /// The roots closed, unless a search opened them all.
    pub fn collapsed_roots(&self) -> Option<Vec<PathBuf>> {
        if self.open_before_search.is_some() {
            return None;
        }
        Some(
            self.roots
                .iter()
                .filter(|root| self.folders.get(*root).is_none_or(|f| !f.open))
                .cloned()
                .collect(),
        )
    }

    /// Lay the rows out again: each root, then what's shown inside it.
    /// While searching, roots without a match are left out.
    pub fn rebuild(&mut self) {
        let mut items = Vec::new();
        for root in self.roots.clone() {
            let searching = !self.query.is_empty();
            if searching
                && self
                    .folders
                    .get(&root)
                    .is_none_or(|f| f.search_result.is_empty())
            {
                continue;
            }
            let node = FileNode {
                name_lower: String::new(),
                path: root,
                depth: 0,
                is_dir: true,
            };
            items.push(node.clone());
            items.extend(self.visible_under(&node));
        }
        self.items = items;
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

    /// Close every folder inside `root`, leaving it open.
    pub fn collapse_all(&mut self, root: &Path) {
        for (path, folder) in &mut self.folders {
            if path != root && path.starts_with(root) {
                folder.open = false;
            }
        }
        self.rebuild();
    }

    /// Show only what matches `query` (case aside): in `scope`, one of the
    /// roots, or in all of them. Matching roots are opened for it.
    #[cfg(test)]
    pub fn search(&mut self, query: &str, scope: Option<&Path>) {
        let mut search = self.start_search();
        search.run(query, scope);
        self.finish_search(query, search);
    }

    /// A search on a copy of the tree, to run without holding it: see
    /// [`Search::run`] and [`Self::finish_search`].
    pub fn start_search(&self) -> Search {
        Search {
            folders: self.folders.clone(),
            roots: self.roots.clone(),
        }
    }

    /// Show the results of `search`, run for `query`: what was read from
    /// disk for it is kept, and folders opened or closed meanwhile stay so.
    pub fn finish_search(&mut self, query: &str, search: Search) {
        let open: HashMap<PathBuf, bool> = self
            .folders
            .iter()
            .map(|(path, folder)| (path.clone(), folder.open))
            .collect();
        self.folders = search.folders;
        for (path, folder) in &mut self.folders {
            if let Some(open) = open.get(path) {
                folder.open = *open;
            }
        }
        self.query = query.to_string();
        if self.open_before_search.is_none() {
            self.open_before_search = Some(
                self.roots
                    .iter()
                    .map(|root| self.folders.get(root).is_some_and(|f| f.open))
                    .collect(),
            );
        }
        for root in &self.roots {
            if let Some(folder) = self.folders.get_mut(root) {
                folder.open = true;
            }
        }
        self.rebuild();
    }

    /// Back to the folders' contents, roots open as before the search.
    pub fn clear_search(&mut self) {
        self.query.clear();
        if let Some(open) = self.open_before_search.take() {
            for (root, open) in self.roots.clone().iter().zip(open) {
                if let Some(folder) = self.folders.get_mut(root) {
                    folder.open = open;
                }
            }
        }
        self.rebuild();
    }
}

/// A search through a copy of the browser's folders, see
/// [`FileTree::start_search`].
pub struct Search {
    folders: HashMap<PathBuf, FolderNode>,
    roots: Vec<PathBuf>,
}

impl Search {
    /// Find what matches `query` (case aside) in `scope`, one of the roots,
    /// or in all of them, reading folders from disk as needed.
    pub fn run(&mut self, query: &str, scope: Option<&Path>) {
        let query = query.to_lowercase();
        for root in &self.roots {
            if scope.is_none_or(|scope| scope == root) {
                filter(root.clone(), &mut self.folders, &query, 0, false);
            } else if let Some(folder) = self.folders.get_mut(root) {
                folder.search_result.clear();
            }
        }
    }
}

/// The folders pinned at the top of `items` scrolled to `first`: those it
/// is in, root first, `max` at most (the outermost). Pinned rows cover the
/// ones under them: an open folder covered inside the last one pinned is
/// pinned too. A pin whose folder ends in view is pushed up by what's
/// next, which is for the caller to draw.
pub fn pinned(items: &[FileNode], first: usize, max: usize) -> Vec<usize> {
    let Some(top) = items.get(first) else {
        return Vec::new();
    };
    let mut chain = ancestors(items, first, top.depth);
    let mut covered = 0;
    while covered < chain.len() && chain.len() < max {
        let row = first + covered;
        let inside_last = chain
            .last()
            .is_some_and(|&last| items[row].depth == items[last].depth + 1);
        let open_folder = items
            .get(row + 1)
            .is_some_and(|next| next.depth > items[row].depth);
        if inside_last && open_folder {
            chain.push(row);
        }
        covered += 1;
    }
    chain.truncate(max);
    chain
}

/// The folders row `index` (at `depth`) is in, outermost first.
fn ancestors(items: &[FileNode], index: usize, depth: usize) -> Vec<usize> {
    let mut chain = Vec::new();
    let mut want = depth;
    for i in (0..index).rev() {
        if want == 0 {
            break;
        }
        if items[i].depth < want {
            chain.push(i);
            want = items[i].depth;
        }
    }
    chain.reverse();
    chain
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Two folders in a fresh temporary directory:
    /// `drums/{kick.wav, loops/break.wav}` and `keys/{pad.wav}`.
    fn folders(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("tonique-tree-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (drums, keys) = (base.join("drums"), base.join("keys"));
        fs::create_dir_all(drums.join("loops")).unwrap();
        fs::create_dir_all(&keys).unwrap();
        for file in [
            drums.join("kick.wav"),
            drums.join("loops/break.wav"),
            keys.join("pad.wav"),
            keys.join("notes.txt"),
        ] {
            fs::write(file, b"").unwrap();
        }
        (base, drums, keys)
    }

    /// The rows shown, as names indented by depth.
    fn rows(tree: &FileTree) -> Vec<String> {
        tree.items
            .iter()
            .map(|f| {
                let name = f.path.file_name().unwrap().to_string_lossy();
                format!("{}{name}", "  ".repeat(f.depth))
            })
            .collect()
    }

    #[test]
    fn each_folder_is_a_root_with_its_contents_below() {
        let (base, drums, keys) = folders("roots");
        let mut tree = FileTree::new();
        tree.set_roots(
            vec![drums.clone(), keys.clone()],
            std::slice::from_ref(&keys),
        );
        assert_eq!(rows(&tree), ["drums", "  kick.wav", "  loops", "keys"]);
        assert_eq!(tree.collapsed_roots(), Some(vec![keys.clone()]));

        // Opening a root shows what's in it: audio files and folders only.
        tree.toggle_folder(3);
        assert_eq!(rows(&tree)[3..], ["keys", "  pad.wav"]);
        assert_eq!(tree.collapsed_roots(), Some(vec![]));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn searching_spans_every_folder_or_one() {
        let (base, drums, keys) = folders("search");
        let mut tree = FileTree::new();
        tree.set_roots(
            vec![drums.clone(), keys.clone()],
            std::slice::from_ref(&drums),
        );

        // A root with no match is left out; one with a match is opened.
        tree.search("BREAK", None);
        assert_eq!(rows(&tree), ["drums", "  loops"]);
        assert_eq!(tree.collapsed_roots(), None, "not while searching");
        tree.search("wav", None);
        assert_eq!(
            rows(&tree),
            ["drums", "  kick.wav", "  loops", "keys", "  pad.wav"]
        );
        // In one folder only.
        tree.search("wav", Some(&keys));
        assert_eq!(rows(&tree), ["keys", "  pad.wav"]);

        // Cleared: the roots closed before are closed again.
        tree.clear_search();
        assert_eq!(rows(&tree), ["drums", "keys", "  pad.wav"]);
        let _ = fs::remove_dir_all(base);
    }

    /// Rows at these depths, as a tree shows them.
    fn tree(depths: &[usize]) -> Vec<FileNode> {
        depths
            .iter()
            .enumerate()
            .map(|(i, &depth)| FileNode {
                path: PathBuf::from(format!("/{i}")),
                depth,
                name_lower: String::new(),
                is_dir: true,
            })
            .collect()
    }

    #[test]
    fn the_folders_scrolled_into_are_pinned_root_first() {
        // 0 root, 1 a, 2 a/b, 3 a/b/file, 4 a/b/file, 5 c, 6 root2
        let items = tree(&[0, 1, 2, 3, 3, 1, 0]);
        assert_eq!(pinned(&items, 0, 4), [] as [usize; 0], "at the top");
        // One row down: the root pinned covers a, pinned in turn covering
        // b, and so on down to the first row left in view (a file of b).
        assert_eq!(pinned(&items, 1, 4), [0, 1, 2]);
        assert_eq!(pinned(&items, 3, 4), [0, 1, 2]);
        // At c: only the root.
        assert_eq!(pinned(&items, 5, 4), [0]);
        assert_eq!(pinned(&items, 3, 2), [0, 1], "the outermost, at most");
        // Into the next root: none of the first's.
        assert_eq!(pinned(&items, 6, 4), [] as [usize; 0]);
    }

    /// A label's section gathers files and folders from anywhere; its
    /// folders open as anywhere else, and a search looks through them.
    #[test]
    fn sections_gather_items_from_anywhere() {
        let (base, drums, keys) = folders("sections");
        let mut tree = FileTree::new();
        let header = PathBuf::from("\u{0}label-0");
        let items = vec![keys.join("pad.wav"), drums.join("loops")];
        tree.set_sections(vec![(header.clone(), items)], &[]);
        assert_eq!(rows(&tree)[1..], ["  loops", "  pad.wav"]);
        tree.toggle_folder(1);
        assert_eq!(rows(&tree)[1..], ["  loops", "    break.wav", "  pad.wav"]);

        // Open, loops shows its match.
        tree.search("break", None);
        assert_eq!(rows(&tree)[1..], ["  loops", "    break.wav"]);
        tree.search("nothing", None);
        assert!(
            tree.items.is_empty(),
            "a section without a match is left out"
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn collapsing_all_closes_a_roots_folders_but_not_it() {
        let (base, drums, _) = folders("collapse");
        let mut tree = FileTree::new();
        tree.set_roots(vec![drums.clone()], &[]);
        tree.toggle_folder(2); // loops
        assert_eq!(
            rows(&tree),
            ["drums", "  kick.wav", "  loops", "    break.wav"]
        );
        tree.collapse_all(&drums);
        assert_eq!(rows(&tree), ["drums", "  kick.wav", "  loops"]);
        let _ = fs::remove_dir_all(base);
    }
}
