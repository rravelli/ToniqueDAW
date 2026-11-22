//! The browser's folders, kept in Tonique's config: added from the
//! system's dialog or dropped from its file manager, removed, shown in it,
//! and read from disk again on F5 or when the window gets the focus back
//! (files are often added from elsewhere meanwhile).

use std::path::PathBuf;

use gpui::{
    AnyElement, Context, DragMoveEvent, ExternalPaths, PathPromptOptions, Stateful, Window, div,
    prelude::*, px,
};

use crate::{
    app::Tonique,
    browser::ListKind,
    theme::{hsla, size, with_alpha},
    widgets::Flat,
};

/// How opaque the accent is over the browser while folders are dragged
/// over it.
const DROP_ALPHA: u8 = 25;

impl Tonique {
    /// Show the folders as configured, searching them again if a search
    /// is on.
    fn reload_folders(&mut self, context: &mut Context<Self>) {
        let config = &self.browser.config;
        let (roots, collapsed) = (config.list_dirs(), config.collapsed_dirs().to_vec());
        self.browser.files.set_roots(roots, &collapsed);
        self.search_again(ListKind::Files, context);
    }

    /// Add `dir` last, open.
    pub(super) fn add_folder(&mut self, dir: PathBuf, context: &mut Context<Self>) {
        self.browser.config.add_dir(dir);
        self.reload_folders(context);
    }

    pub(super) fn remove_folder(&mut self, dir: &std::path::Path, context: &mut Context<Self>) {
        self.browser.config.remove_dir(dir);
        self.reload_folders(context);
    }

    /// Pick folders to add, from the system's dialog.
    pub(super) fn pick_folder(&mut self, window: &mut Window, context: &mut Context<Self>) {
        let picked = context.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Add".into()),
        });
        context
            .spawn_in(window, async move |this, context| {
                let Ok(Ok(Some(paths))) = picked.await else {
                    return;
                };
                this.update(context, |this, context| {
                    for path in paths {
                        this.add_folder(path, context);
                    }
                    context.notify();
                })
                .ok();
            })
            .detach();
    }

    /// Read both lists' folders from disk again, keeping the row selected
    /// and the searches; labelled files gone are left out.
    pub fn refresh_browser(&mut self, context: &mut Context<Self>) {
        let files = &mut self.browser.files;
        let selected = files.selected_path().map(|path| path.to_path_buf());
        files.tree.refresh();
        files.selected =
            selected.and_then(|path| files.tree.items.iter().position(|item| item.path == path));
        self.browser.refresh_labels();
        self.browser.labelled.tree.refresh();
        self.search_again(ListKind::Files, context);
        self.search_again(ListKind::Labels, context);
    }

    /// `panel` taking folders dropped from the system's file manager, lit
    /// while they're over it.
    pub(super) fn take_folder_drops(
        &self,
        panel: gpui::Div,
        context: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let theme = &self.theme;
        panel
            .id("browser-drop")
            .relative()
            .on_drag_move(
                context.listener(|this, _: &DragMoveEvent<ExternalPaths>, _, context| {
                    if !this.browser.folders_over {
                        this.browser.folders_over = true;
                        context.notify();
                    }
                }),
            )
            .on_drop(context.listener(|this, paths: &ExternalPaths, _, context| {
                this.browser.folders_over = false;
                for path in paths.paths().iter().filter(|path| path.is_dir()) {
                    this.add_folder(path.clone(), context);
                }
                context.notify();
            }))
            .when(self.browser.folders_over, |panel| {
                panel.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.))
                        .border_1()
                        .border_color(hsla(theme.accent))
                        .bg(hsla(with_alpha(theme.accent, DROP_ALPHA)))
                        .text_color(hsla(theme.text))
                        .child("Drop folders to add them"),
                )
            })
    }

    /// No folder yet: what the browser is for, and a button to add one.
    pub(super) fn no_folders_view(&self, context: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        div()
            .pt(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .child(
                div()
                    .text_color(hsla(theme.text_muted))
                    .child("Your sample folders show here"),
            )
            .child(
                div()
                    .text_size(px(size::SMALL))
                    .text_color(hsla(theme.text_disabled))
                    .child("Drop one here, or"),
            )
            .child(div().h(px(4.)))
            .child(
                Flat::new("add-first-folder", 24.)
                    .fit(10.)
                    .element("Add a folder", theme)
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        context.listener(|this, _, window, context| {
                            this.pick_folder(window, context);
                        }),
                    ),
            )
            .into_any_element()
    }
}
