//! What can be done with a row of the browser's, from its menu: a folder
//! of the browser's, a label's section, or a file or folder inside one.

use std::path::{Path, PathBuf};

use gpui::{Anchor, Context, MouseButton, Pixels, Point, Window, div, prelude::*, px};

use crate::{
    app::PopupKind,
    browser::{BrowserPanel, ListKind, rows::dot},
    icons,
    popup::{self, MenuBuilder},
    theme::hsla,
    widgets::{FieldKey, Menu, MenuItem},
};
use tonique_core::{config::labels as label_colors, utils::color::parse_color};

/// A colour of a label's menu, and the ring around the one it has.
const SWATCH: f32 = 12.;
const SWATCH_RING: f32 = 16.;

impl BrowserPanel {
    /// Builds a row's menu, or its labels beside it, each frame.
    pub(super) fn menu_builder(&self, kind: PopupKind, context: &Context<Self>) -> MenuBuilder {
        popup::builder(context.weak_entity(), move |this, _, context| match kind {
            PopupKind::BrowserRow(list, index) => this.browser_row_menu(list, index, context),
            PopupKind::BrowserRowLabel(list, index) => {
                this.browser_label_menu(list, index, context)
            }
            _ => None,
        })
    }

    /// Opens a row's menu with its `anchor` corner at `at`; or, when it's
    /// open, closes it.
    pub(super) fn toggle_popup(
        &mut self,
        kind: PopupKind,
        at: Point<Pixels>,
        anchor: Anchor,
        context: &mut Context<Self>,
    ) {
        let build = self.menu_builder(kind, context);
        popup::toggle(kind, at, anchor, build, context);
    }

    /// Row `index` of `kind`'s menu: a folder's, a label's or an item's.
    pub(crate) fn browser_row_menu(
        &self,
        kind: ListKind,
        index: usize,
        context: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let item = self.list(kind).tree.items.get(index)?;
        Some(if let Some((label, _)) = self.row_section(kind, index) {
            self.section_menu(label, context)
        } else if item.depth == 0 {
            self.folder_menu(kind, index, &item.path.clone(), context)
        } else {
            self.item_menu(kind, index, context)?
        })
    }

    /// Whether `kind`'s menu is shown beside an item of the open menu.
    fn browser_submenu_open(&self, kind: PopupKind, context: &gpui::App) -> bool {
        crate::popup::submenu_shown(context) == Some(kind)
    }

    /// One of the browser's folders: searching in it alone, refreshing,
    /// collapsing, showing it, its label, and removing it.
    fn folder_menu(
        &self,
        kind: ListKind,
        index: usize,
        path: &Path,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let scoped = self.files.scope.as_deref() == Some(path);
        let owned = path.to_path_buf();
        let search = if scoped {
            MenuItem::new("Search all folders")
        } else {
            MenuItem::new("Search in this folder")
        };
        let label = PopupKind::BrowserRowLabel(kind, index);
        Menu::new(&crate::theme::current(context))
            .item(search.icon(icons::MAGNIFYING_GLASS), context, {
                let path = owned.clone();
                move |this, _, context| {
                    let scope = (!scoped).then(|| path.clone());
                    this.set_search_scope(scope, context);
                }
            })
            .item(
                MenuItem::new("Refresh")
                    .icon(icons::ARROWS_CLOCKWISE)
                    .shortcut("F5"),
                context,
                |this, _, context| this.refresh_browser(context),
            )
            .item(
                MenuItem::new("Collapse all").icon(icons::ARROWS_IN_LINE_VERTICAL),
                context,
                {
                    let path = owned.clone();
                    move |this, _, _| {
                        let files = &mut this.files;
                        files.tree.collapse_all(&path);
                        files.selected = None;
                    }
                },
            )
            .item(
                MenuItem::new("Show in file manager").icon(icons::ARROW_SQUARE_OUT),
                context,
                {
                    let path = owned.clone();
                    move |_, _, context| open_with_system(&path, context)
                },
            )
            .submenu(
                MenuItem::new("Label").icon(icons::TAG),
                label,
                self.browser_submenu_open(label, context),
                self.menu_builder(label, context),
            )
            .separator()
            .item(
                MenuItem::new("Remove from browser")
                    .icon(icons::FOLDER_MINUS)
                    .color(crate::theme::current(context).danger),
                context,
                move |this, _, context| this.remove_folder(&owned, context),
            )
            .into_element()
    }

    /// A file or folder in a section: its label, adding a folder as one of
    /// the browser's, showing it.
    fn item_menu(
        &self,
        kind: ListKind,
        index: usize,
        context: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let item = self.list(kind).tree.items.get(index)?.clone();
        let label = PopupKind::BrowserRowLabel(kind, index);
        // Top level of a label's section: added from its own folder.
        let addable = item.is_dir && kind == ListKind::Files;
        let shown: Option<PathBuf> = if item.is_dir {
            Some(item.path.clone())
        } else {
            item.path.parent().map(Path::to_path_buf)
        };
        let mut menu = Menu::new(&crate::theme::current(context)).submenu(
            MenuItem::new("Label").icon(icons::TAG),
            label,
            self.browser_submenu_open(label, context),
            self.menu_builder(label, context),
        );
        if addable {
            let path = item.path.clone();
            menu = menu.item(
                MenuItem::new("Add as browser folder").icon(icons::FOLDER_PLUS),
                context,
                move |this, _, context| this.add_folder(path.clone(), context),
            );
        }
        if let Some(shown) = shown {
            menu = menu.item(
                MenuItem::new("Show in file manager").icon(icons::ARROW_SQUARE_OUT),
                context,
                move |_, _, context| open_with_system(&shown, context),
            );
        }
        Some(menu.into_element())
    }

    /// The labels, beside a row's menu: to give it one (1 to 7) or none
    /// (0). Its own label again takes it off.
    pub(crate) fn browser_label_menu(
        &self,
        kind: ListKind,
        index: usize,
        context: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let path = self.list(kind).tree.items.get(index)?.path.clone();
        let current = self.item_labels.get(&path).copied();
        let mut menu = Menu::new_submenu(&crate::theme::current(context));
        for (label, view) in self.labels.iter().enumerate() {
            let chosen = current == Some(label);
            let item = MenuItem::new(view.name.clone())
                .swatch(view.color)
                .checked(chosen)
                .shortcut((label + 1).to_string());
            let path = path.clone();
            menu = menu.item(item, context, move |this, _, context| {
                this.set_label(path.clone(), (!chosen).then_some(label), context);
            });
        }
        Some(
            menu.separator()
                .item(
                    MenuItem::new("No label")
                        .checked(current.is_none())
                        .shortcut("0"),
                    context,
                    move |this, _, context| this.set_label(path.clone(), None, context),
                )
                .into_element(),
        )
    }

    /// Label `index`'s: its name, typed in the menu (Enter renames it), its
    /// colour, and taking it off everything.
    fn section_menu(&self, index: usize, context: &mut Context<Self>) -> gpui::Div {
        let theme = &*crate::theme::current(context);
        let current = self.labels.get(index).map(|label| label.color);
        let field = &self.label_name;
        let name = div()
            .id("label-name")
            .track_focus(&field.focus)
            .mx(px(4.))
            .my(px(2.))
            .px(px(4.))
            .h(px(22.))
            .flex()
            .items_center()
            .rounded(px(2.))
            .bg(hsla(theme.bg_deep))
            .border_1()
            .border_color(hsla(theme.accent))
            .on_key_down(context.listener(move |this, event, window, context| {
                this.label_name_key(index, event, window, context);
            }))
            .child(field.view_focused());
        let swatches = label_colors::COLORS
            .iter()
            .enumerate()
            .filter_map(|(slot, hex)| {
                let color = parse_color(hex)?;
                let ring = current == Some(color);
                Some(
                    div()
                        .id(("swatch", slot))
                        .size(px(SWATCH_RING))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .border_1()
                        .border_color(hsla(if ring {
                            theme.text
                        } else {
                            crate::theme::Color::rgba(0, 0, 0, 0)
                        }))
                        .hover(|style| style.border_color(hsla(theme.text)))
                        .cursor_pointer()
                        .child(dot(color, SWATCH))
                        .on_mouse_down(
                            MouseButton::Left,
                            context.listener(move |this, _, _, context| {
                                context.stop_propagation();
                                this.recolor_label(index, hex, context);
                                context.notify();
                            }),
                        ),
                )
            });
        Menu::new(theme)
            .child(name)
            .child(
                div()
                    .max_w(px(5. * (SWATCH_RING + 4.)))
                    .mx(px(4.))
                    .my(px(4.))
                    .flex()
                    .flex_wrap()
                    .gap(px(4.))
                    .children(swatches),
            )
            .separator()
            .item(
                MenuItem::new("Clear label")
                    .icon(icons::ERASER)
                    .color(theme.danger),
                context,
                move |this, _, context| this.clear_label(index, context),
            )
            .into_element()
    }

    /// A key typed in label `index`'s name: Enter renames it and closes
    /// the menu, Escape closes it.
    fn label_name_key(
        &mut self,
        index: usize,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        match self.label_name.key(event) {
            FieldKey::Submit => {
                let name = self.label_name.text.trim().to_string();
                if !name.is_empty() {
                    self.rename_label(index, name, context);
                }
                self.close_browser_menu(window, context);
            }
            FieldKey::Cancel => self.close_browser_menu(window, context),
            FieldKey::Edited => {}
            FieldKey::Ignored => return,
        }
        context.stop_propagation();
        context.notify();
    }

    fn close_browser_menu(&mut self, window: &mut Window, context: &mut Context<Self>) {
        crate::popup::close(context);
        window.focus(&self.list_focus, context);
    }
}

/// Opens `path` with the system's app for it: a folder in the file
/// manager. Not in tests: gpui-fast's test platform has no system to ask.
fn open_with_system(path: &std::path::Path, context: &gpui::App) {
    if cfg!(test) {
        return;
    }
    context.open_with_system(path);
}
