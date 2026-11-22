//! A list of the browser's: the folders' tree or the labels' sections,
//! searched in the background, browsed with the mouse or the keys. While
//! scrolled into folders, the way down to the first row shown stays
//! pinned at the top; a scrollbar runs down the right.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use gpui::{
    AnyElement, Context, KeyDownEvent, ScrollStrategy, Task, UniformListScrollHandle, Window, div,
    prelude::*, px, uniform_list,
};

use crate::{
    app::Tonique,
    cache::AUDIO_ANALYSIS_CACHE,
    file_tree::{FileTree, is_audio_file, pinned},
    theme::hsla,
    widgets::{FieldKey, HANDLE_WIDTH, ScrollbarGrab, TextField, handle_scrollbar},
};

use super::rows::ROW_HEIGHT;

/// Most rows pinned at the top while scrolled into folders.
const MAX_PINNED: usize = 4;

/// Which of the browser's lists.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ListKind {
    /// The folders added to the browser.
    Files,
    /// What has a label, by label.
    Labels,
}

pub struct FileList {
    pub(super) tree: FileTree,
    pub(super) selected: Option<usize>,
    pub(super) scroll: UniformListScrollHandle,
    scrollbar: ScrollbarGrab,
    pub(super) search: TextField,
    /// The search running in the background; the latest wins.
    search_task: Option<Task<()>>,
    /// The folder searched in, or all of them.
    pub(super) scope: Option<PathBuf>,
    /// Each folder's name as shown: its own, and its parent's when another
    /// folder has the same name.
    pub(super) names: HashMap<PathBuf, String>,
}

impl FileList {
    pub fn new(context: &mut Context<Tonique>) -> Self {
        Self {
            tree: FileTree::new(),
            selected: None,
            scroll: UniformListScrollHandle::new(),
            scrollbar: ScrollbarGrab::default(),
            search: TextField::new(context),
            search_task: None,
            scope: None,
            names: HashMap::new(),
        }
    }

    /// Show `roots`, all but `collapsed` open.
    pub fn set_roots(&mut self, roots: Vec<PathBuf>, collapsed: &[PathBuf]) {
        self.selected = None;
        self.names = folder_names(&roots);
        if self
            .scope
            .as_ref()
            .is_some_and(|scope| !roots.contains(scope))
        {
            self.scope = None;
        }
        self.tree.set_roots(roots, collapsed);
    }

    /// Whether row `index` is an open folder.
    pub(super) fn is_open(&self, index: usize) -> bool {
        let items = &self.tree.items;
        items.get(index).is_some_and(|item| {
            item.is_dir
                && items
                    .get(index + 1)
                    .is_some_and(|next| next.depth > item.depth)
        })
    }

    pub(super) fn is_searching(&self) -> bool {
        self.search_task.is_some()
    }

    /// The name of the folder the search is kept to, if it is.
    pub(super) fn scope_name(&self) -> Option<String> {
        self.names.get(self.scope.as_ref()?).cloned()
    }

    fn query(&self) -> String {
        self.search.text.trim().to_string()
    }

    /// The path of the row selected.
    pub(super) fn selected_path(&self) -> Option<&Path> {
        Some(self.tree.items.get(self.selected?)?.path.as_path())
    }
}

/// Each folder's name as shown: its own, or with its parent's after it
/// (`Samples — Splice`) when another has the same name.
fn folder_names(roots: &[PathBuf]) -> HashMap<PathBuf, String> {
    let name = |path: &Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string())
    };
    roots
        .iter()
        .map(|root| {
            let own = name(root);
            let shared = roots.iter().filter(|other| name(other) == own).count() > 1;
            let shown = match root.parent().filter(|_| shared) {
                Some(parent) => format!("{own} — {}", name(parent)),
                None => own,
            };
            (root.clone(), shown)
        })
        .collect()
}

impl Tonique {
    /// Select row `index` of `kind`: open or close a folder, or select (and
    /// preview) a file.
    pub(super) fn browser_select(&mut self, kind: ListKind, index: usize, toggle_folder: bool) {
        let list = self.browser.list_mut(kind);
        let Some(item) = list.tree.items.get(index).cloned() else {
            return;
        };
        list.selected = Some(index);
        if item.is_dir {
            if toggle_folder {
                list.tree.toggle_folder(index);
                self.remember_collapsed(kind);
            }
            return;
        }
        self.browser.audio = None;
        if !is_audio_file(&item.path) {
            return;
        }
        self.browser.audio = AUDIO_ANALYSIS_CACHE.get_or_analyze(item.path.clone());
        if let Some(audio) = &self.browser.audio {
            audio.load_peaks();
        }
        if self.browser.auto_preview {
            self.state.play_preview(item.path);
        } else if self.state.previewing().is_some() {
            // Another file's.
            self.state.pause_preview();
        }
    }

    /// Folders opened or closed: remembered for next time (the labels'
    /// sections until the app closes).
    pub(super) fn remember_collapsed(&mut self, kind: ListKind) {
        let Some(collapsed) = self.browser.list(kind).tree.collapsed_roots() else {
            return;
        };
        match kind {
            ListKind::Files => self.browser.config.set_collapsed_dirs(collapsed),
            ListKind::Labels => self.browser.labels_collapsed = collapsed,
        }
    }

    /// The keys of the list shown, while it has the keyboard: the arrows,
    /// Enter, 0 to 7 for labels and F5. Returns whether the key was the
    /// list's.
    pub fn browser_key(&mut self, event: &KeyDownEvent, context: &mut Context<Self>) -> bool {
        let Some(kind) = self.browser.shown_list() else {
            return false;
        };
        let keystroke = &event.keystroke;
        let modifiers = &keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform {
            return false;
        }
        let key = keystroke.key.as_str();
        if key == "f5" {
            self.refresh_browser(context);
            context.notify();
            return true;
        }
        if let Some(digit) = key.parse::<usize>().ok().filter(|digit| *digit <= 7) {
            let taken = self.label_key(kind, digit, context);
            context.notify();
            return taken;
        }
        let len = self.browser.list(kind).tree.items.len();
        if len == 0 || !matches!(key, "down" | "up" | "right" | "left" | "enter") {
            return false;
        }
        let selected = self.browser.list(kind).selected;
        match key {
            "down" => self.browser_select(
                kind,
                selected.map_or(0, |index| (index + 1).min(len - 1)),
                false,
            ),
            "up" => self.browser_select(
                kind,
                selected.map_or(0, |index| index.saturating_sub(1)),
                false,
            ),
            _ => {
                let Some(index) = selected else { return true };
                self.browser_open_key(kind, index, key);
            }
        }
        let list = self.browser.list(kind);
        if let Some(index) = list.selected {
            list.scroll.scroll_to_item(index, ScrollStrategy::Center);
        }
        context.notify();
        true
    }

    /// Right opens a folder, Left closes it (or goes to the folder a file
    /// is in), Enter toggles a folder or plays or stops a file.
    fn browser_open_key(&mut self, kind: ListKind, index: usize, key: &str) {
        let list = self.browser.list_mut(kind);
        let Some(node) = list.tree.items.get(index).cloned() else {
            return;
        };
        let open = list.is_open(index);
        match key {
            "right" | "left" | "enter" if node.is_dir => {
                let toggles = match key {
                    "right" => !open,
                    "left" => open,
                    _ => true,
                };
                if toggles {
                    list.tree.toggle_folder(index);
                    self.remember_collapsed(kind);
                } else if key == "left" {
                    select_parent(list, index, node.depth);
                }
            }
            "left" => select_parent(list, index, node.depth),
            "enter" if is_audio_file(&node.path) => {
                // Whether or not selecting plays.
                if self.state.previewing() == Some(node.path.as_path()) {
                    self.state.pause_preview();
                } else {
                    self.state.play_preview(node.path);
                }
            }
            _ => {}
        }
    }

    fn search_key(
        &mut self,
        kind: ListKind,
        event: &KeyDownEvent,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let search = &mut self.browser.list_mut(kind).search;
        match event.keystroke.key.as_str() {
            "down" => window.focus(&self.browser.list_focus, context),
            _ => match search.key(event) {
                FieldKey::Edited => self.run_search(kind, context),
                FieldKey::Submit => window.focus(&self.browser.list_focus, context),
                FieldKey::Cancel => {
                    search.text.clear();
                    window.focus(&self.browser.list_focus, context);
                    self.run_search(kind, context);
                }
                FieldKey::Ignored => return,
            },
        }
        context.stop_propagation();
        context.notify();
    }

    fn files_search_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        self.search_key(ListKind::Files, event, window, context);
    }

    fn labels_search_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        self.search_key(ListKind::Labels, event, window, context);
    }

    /// Search `kind`'s folders (or the one searched in) in the background;
    /// the latest search wins.
    pub(super) fn run_search(&mut self, kind: ListKind, context: &mut Context<Self>) {
        let list = self.browser.list_mut(kind);
        let query = list.query();
        list.selected = None;
        if query.is_empty() {
            list.search_task = None;
            list.tree.clear_search();
            return;
        }
        let scope = list.scope.clone();
        let mut search = list.tree.start_search();
        list.search_task = Some(context.spawn(async move |this, context| {
            let searched = query.clone();
            let search = context
                .background_executor()
                .spawn(async move {
                    search.run(&searched, scope.as_deref());
                    search
                })
                .await;
            this.update(context, |this, context| {
                let list = this.browser.list_mut(kind);
                if list.query() == query {
                    list.tree.finish_search(&query, search);
                    list.search_task = None;
                    context.notify();
                }
            })
            .ok();
        }));
    }

    /// Search again if a search is on: what's listed changed.
    pub(super) fn search_again(&mut self, kind: ListKind, context: &mut Context<Self>) {
        if !self.browser.list(kind).query().is_empty() {
            self.run_search(kind, context);
        }
    }

    /// Search in `scope` only, or (`None`) in every folder.
    pub(super) fn set_search_scope(&mut self, scope: Option<PathBuf>, context: &mut Context<Self>) {
        self.browser.files.scope = scope;
        self.search_again(ListKind::Files, context);
    }

    fn clear_files_search(&mut self, context: &mut Context<Self>) {
        self.browser.files.search.text.clear();
        self.run_search(ListKind::Files, context);
    }

    fn clear_labels_search(&mut self, context: &mut Context<Self>) {
        self.browser.labelled.search.text.clear();
        self.run_search(ListKind::Labels, context);
    }

    /// `kind`'s search bar and list.
    pub(super) fn list_view(
        &mut self,
        kind: ListKind,
        window: &mut Window,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.theme.clone();
        let list = self.browser.list(kind);
        let (placeholder, key, clear): (String, _, fn(&mut Self, &mut Context<Self>)) = match kind {
            ListKind::Files => (
                match list.scope_name() {
                    Some(folder) => format!("Search in {folder}"),
                    None => "Search all folders".into(),
                },
                Self::files_search_key
                    as fn(&mut Self, &KeyDownEvent, &mut Window, &mut Context<Self>),
                Self::clear_files_search,
            ),
            ListKind::Labels => (
                "Search labelled files".into(),
                Self::labels_search_key as _,
                Self::clear_labels_search,
            ),
        };
        let id = match kind {
            ListKind::Files => "file-search",
            ListKind::Labels => "label-search",
        };
        let search_bar = self.search_bar(
            id,
            &list.search,
            &placeholder,
            list.is_searching(),
            &theme,
            window,
            context,
            key,
            clear,
        );
        let body = if kind == ListKind::Files && self.browser.config.list_dirs().is_empty() {
            self.no_folders_view(context)
        } else if list.tree.items.is_empty() && !list.query().is_empty() && !list.is_searching() {
            self.nothing_found_view(kind, context)
        } else {
            self.rows_view(kind, context)
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(search_bar)
            .child(
                div()
                    .id(match kind {
                        ListKind::Files => "file-list",
                        ListKind::Labels => "label-list",
                    })
                    .track_focus(&self.browser.list_focus)
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(body),
            )
            .into_any_element()
    }

    /// A search found nothing: says so, and in a folder, offers to search
    /// them all.
    fn nothing_found_view(&self, kind: ListKind, context: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let scope = self.browser.list(kind).scope_name();
        let text = match &scope {
            Some(folder) => format!("No files match in {folder}"),
            None => "No files match".into(),
        };
        div()
            .p_3()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .text_color(hsla(theme.text_muted))
            .child(text)
            .when(scope.is_some(), |element| {
                element.child(
                    div()
                        .id("search-everywhere")
                        .text_color(hsla(theme.accent))
                        .cursor_pointer()
                        .hover(|style| style.underline())
                        .child("Search all folders")
                        .on_click(context.listener(|this, _, _, context| {
                            this.set_search_scope(None, context);
                            context.notify();
                        })),
                )
            })
            .into_any_element()
    }

    /// The rows, the last one adding a folder (in the files' list, unless
    /// it shows search results), and the folders pinned over them.
    fn rows_view(&self, kind: ListKind, context: &mut Context<Self>) -> AnyElement {
        let list = self.browser.list(kind);
        let count = list.tree.items.len();
        let add_row = kind == ListKind::Files && list.query().is_empty();
        let rows = uniform_list(
            match kind {
                ListKind::Files => "files",
                ListKind::Labels => "labelled",
            },
            count + usize::from(add_row),
            context.processor(move |this, range: std::ops::Range<usize>, _, context| {
                range
                    .map(|index| {
                        if index == count {
                            this.add_folder_row(context)
                        } else {
                            this.browser_row(kind, index, false, context)
                        }
                    })
                    .collect()
            }),
        )
        .track_scroll(&list.scroll)
        .size_full();
        let handle = list.scroll.0.borrow().base_handle.clone();
        div()
            .relative()
            .size_full()
            .child(rows)
            .children(self.pinned_rows(kind, context))
            .child(handle_scrollbar(
                &handle,
                &list.scrollbar,
                // Clear of the browser's resize grip, down the same edge.
                HANDLE_WIDTH,
                &self.theme,
                context,
            ))
            .into_any_element()
    }

    /// Scrolled into folders: the way down to the first row shown (a
    /// folder's header, the subfolders it's in) at the top, each pushed up
    /// by the first row after its contents.
    fn pinned_rows(&self, kind: ListKind, context: &mut Context<Self>) -> Vec<AnyElement> {
        let list = self.browser.list(kind);
        let items = &list.tree.items;
        let offset = f32::from(list.scroll.0.borrow().base_handle.offset().y);
        let first = (-offset / ROW_HEIGHT).floor().max(0.) as usize;
        let chain = pinned(items, first, MAX_PINNED);
        let mut tops: Vec<f32> = Vec::with_capacity(chain.len());
        for (level, &index) in chain.iter().enumerate() {
            let slot = level as f32 * ROW_HEIGHT;
            let end =
                (index + 1..items.len()).find(|&next| items[next].depth <= items[index].depth);
            let pushed = end.map_or(slot, |end| {
                slot.min(end as f32 * ROW_HEIGHT + offset - ROW_HEIGHT)
            });
            // Under its parent, moved up with it.
            let under = level
                .checked_sub(1)
                .map_or(pushed, |parent| pushed.min(tops[parent] + ROW_HEIGHT));
            tops.push(under);
        }
        let last = chain.len().saturating_sub(1);
        // Deepest first: a pushed up subfolder slides under its parent.
        chain
            .iter()
            .enumerate()
            .rev()
            .map(|(level, &index)| {
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(tops[level]))
                    .bg(hsla(self.theme.bg_panel))
                    .when(level == last, |element| {
                        element
                            .border_b_1()
                            .border_color(hsla(self.theme.separator))
                    })
                    .child(self.browser_row(kind, index, true, context))
                    .into_any_element()
            })
            .collect()
    }
}

/// Selects the folder row `index` (at `depth`) is in.
fn select_parent(list: &mut FileList, index: usize, depth: usize) {
    let items = &list.tree.items;
    if let Some(parent) = (0..index)
        .rev()
        .find(|&candidate| items[candidate].depth < depth)
    {
        list.selected = Some(parent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_of_the_same_name_tell_their_parents() {
        let roots = [
            PathBuf::from("/a/Splice/Samples"),
            PathBuf::from("/b/Drums/Samples"),
            PathBuf::from("/c/Loops"),
        ];
        let names = folder_names(&roots);
        assert_eq!(names[&roots[0]], "Samples — Splice");
        assert_eq!(names[&roots[1]], "Samples — Drums");
        assert_eq!(names[&roots[2]], "Loops");
    }
}
