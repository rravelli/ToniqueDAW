//! A row of a list: a section's header (one of the browser's folders, or a
//! label's), a folder, an audio file to drag onto the timeline, or another
//! file, dimmed; and the row ending the folders, adding one.

use super::BrowserPanel;
use crate::model::HoldsProject;
use gpui::{
    Anchor, AnyElement, App, Context, MouseButton, MouseDownEvent, Stateful, div, prelude::*, px,
};

use crate::{
    app::PopupKind,
    browser::{DragGhost, DraggedAudio, ListKind},
    icons,
    theme::{Theme, hsla, icon, mix, size},
};
use tonique_core::{
    core::state::PlaybackState,
    file_tree::{FileKind, FileNode, file_kind},
};

pub(super) const ROW_HEIGHT: f32 = 22.;
const INDENT: f32 = 12.;
/// How much of the accent a selected row is tinted with.
const SELECTED_TINT: f32 = 0.25;
/// How much a row lightens when hovered.
const HOVER_TINT: f32 = 0.05;
/// A label's dot, at the end of a row.
const DOT: f32 = 6.;
/// The hover group of a header, showing its menu's button.
const HEADER_GROUP: &str = "browser-header";

/// A dot of `color`, `diameter` wide.
pub(super) fn dot(color: crate::theme::Color, diameter: f32) -> gpui::Div {
    div()
        .flex_none()
        .size(px(diameter))
        .rounded_full()
        .bg(hsla(color))
}

impl BrowserPanel {
    /// Row `index` of `kind`; `pinned` over the list, at its top.
    pub(super) fn browser_row(
        &self,
        kind: ListKind,
        index: usize,
        pinned: bool,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let list = self.list(kind);
        let item = list.tree.items[index].clone();
        let selected = list.selected == Some(index);
        let open = list.is_open(index);
        let id = if pinned {
            ("pinned-row", index)
        } else {
            ("row", index)
        };
        let theme = &*crate::theme::current(context);
        let base = if item.depth == 0 {
            theme.bg_raised
        } else {
            theme.bg_panel
        };
        let background = if selected {
            mix(base, theme.accent, SELECTED_TINT)
        } else {
            base
        };
        let row = div()
            .id(id)
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap_1()
            .pr_1()
            .bg(hsla(background))
            .hover(move |style| style.bg(hsla(mix(background, theme.text, HOVER_TINT))))
            .whitespace_nowrap()
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, _: &MouseDownEvent, window, context| {
                    window.focus(&this.list_focus, context);
                    this.browser_select(kind, index, true, context);
                    context.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                context.listener(move |this, event: &MouseDownEvent, window, context| {
                    window.focus(&this.list_focus, context);
                    this.list_mut(kind).selected = Some(index);
                    this.open_row_menu(kind, index, event, window, context);
                    context.notify();
                }),
            )
            .child(caret(item.is_dir, open, theme));
        if item.depth == 0 {
            self.header_row(row, kind, index, &item, context)
        } else {
            self.item_row(row, &item, open, context)
        }
    }

    /// Opens row `index`'s menu at the pointer; a label's starts with its
    /// name ready to be typed over.
    fn open_row_menu(
        &mut self,
        kind: ListKind,
        index: usize,
        event: &MouseDownEvent,
        window: &mut gpui::Window,
        context: &mut Context<Self>,
    ) {
        let menu = PopupKind::BrowserRow(kind, index);
        self.toggle_popup(menu, event.position, Anchor::TopLeft, context);
        let opened = crate::popup::shown(context) == Some(menu);
        if opened && let Some((label, _)) = self.row_section(kind, index) {
            let name = self.labels[label].name.clone();
            self.label_name.start(name, window, context);
        }
    }

    /// A section's header, a band across the list: one of the browser's
    /// folders (its name in small capitals) or a label's (its colour, name
    /// and how many items it has), with its menu's button while hovered.
    fn header_row(
        &self,
        row: Stateful<gpui::Div>,
        kind: ListKind,
        index: usize,
        item: &FileNode,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let theme = &*crate::theme::current(context);
        let list = self.list(kind);
        let section = self.sections.get(&item.path).copied();
        let scoped = kind == ListKind::Files && list.scope.as_ref() == Some(&item.path);
        let name = match section {
            Some((label, _)) => self.labels.get(label).map(|label| label.name.clone()),
            None => list.names.get(&item.path).cloned(),
        }
        .unwrap_or_default()
        .to_uppercase();
        let empty = section.is_some_and(|(_, count)| count == 0);
        let section_color = section
            .and_then(|(label, _)| self.labels.get(label))
            .map(|label| label.color);
        let folder_label = section
            .is_none()
            .then(|| self.label_of(&item.path))
            .flatten();
        let menu_button = div()
            .id("header-menu")
            .flex_none()
            .size(px(ROW_HEIGHT - 4.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(2.))
            .text_color(hsla(theme.text_muted))
            .opacity(0.)
            .group_hover(HEADER_GROUP, |style| style.opacity(1.))
            .hover(|style| style.bg(hsla(theme.hover_overlay)))
            .cursor_pointer()
            .child(icon(icons::DOTS_THREE))
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, event: &MouseDownEvent, window, context| {
                    context.stop_propagation();
                    this.open_row_menu(kind, index, event, window, context);
                    context.notify();
                }),
            );
        let clear_scope = scoped.then(|| {
            div()
                .id("clear-scope")
                .flex_none()
                .size(px(ROW_HEIGHT - 4.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(2.))
                .text_color(hsla(theme.text_muted))
                .hover(|style| style.bg(hsla(theme.hover_overlay)))
                .cursor_pointer()
                .child(icon(icons::X))
                .on_mouse_down(
                    MouseButton::Left,
                    context.listener(|this, _, _, context| {
                        context.stop_propagation();
                        this.set_search_scope(None, context);
                        context.notify();
                    }),
                )
        });
        row.group(HEADER_GROUP)
            .pl(px(6.))
            .when(index > 0, |row| {
                row.border_t_1().border_color(hsla(theme.separator))
            })
            .children(section_color.map(|color| dot(color, 8.)))
            .when(scoped, |row| {
                row.child(icon(icons::MAGNIFYING_GLASS).text_color(hsla(theme.accent)))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_size(px(size::SMALL))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(hsla(if empty {
                        theme.text_disabled
                    } else {
                        theme.text
                    }))
                    .child(name),
            )
            .children(folder_label.map(|label| dot(label.color, DOT)))
            .children(section.map(|(_, count)| {
                div()
                    .flex_none()
                    .text_size(px(size::SMALL))
                    .text_color(hsla(theme.text_disabled))
                    .child(count.to_string())
            }))
            .children(clear_scope)
            .child(menu_button)
            .into_any_element()
    }

    /// A folder or file inside a section: its icon (a speaker while it's
    /// previewed), its name, and its label's dot.
    fn item_row(
        &self,
        row: Stateful<gpui::Div>,
        item: &FileNode,
        open: bool,
        context: &App,
    ) -> AnyElement {
        let theme = crate::theme::current(context);
        let name = item.path.file_name().map_or_else(
            || item.path.display().to_string(),
            |file_name| file_name.to_string_lossy().into_owned(),
        );
        let kind = (!item.is_dir).then(|| file_kind(&item.path)).flatten();
        let audio = kind == Some(FileKind::Audio);
        let previewing = self.state(context).previewing() == Some(item.path.as_path())
            && self.state(context).preview_playback_state() == PlaybackState::Playing;
        let glyph = match (item.is_dir, open, kind) {
            _ if previewing => icons::SPEAKER_HIGH,
            (true, true, _) => icons::FOLDER_OPEN,
            (true, false, _) => icons::FOLDER,
            (false, _, Some(FileKind::Audio)) => icons::FILE_AUDIO,
            (false, _, Some(FileKind::Midi)) => icons::PIANO_KEYS,
            (false, _, Some(FileKind::UnsupportedAudio)) => icons::FILE_X,
            (false, _, None) => icons::FILE,
        };
        let tint = if audio || previewing {
            theme.accent
        } else {
            theme.text_muted
        };
        let color = if item.is_dir || audio {
            theme.text
        } else {
            theme.text_disabled
        };
        let label = self.label_of(&item.path);
        let row = row
            .pl(px(8. + item.depth as f32 * INDENT))
            .text_color(hsla(color))
            .child(icon(glyph).text_color(hsla(tint)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(name.clone()),
            )
            .children(label.map(|label| dot(label.color, DOT)));
        if !audio {
            return row.into_any_element();
        }
        let dragged = DraggedAudio {
            path: item.path.clone(),
        };
        row.on_drag(dragged, move |_, _, _, context| {
            context.new(|_| DragGhost {
                name: name.clone().into(),
            })
        })
        .into_any_element()
    }

    /// The row ending the folders: adds one, picked on disk.
    pub(super) fn add_folder_row(&self, context: &mut Context<Self>) -> AnyElement {
        let theme = &*crate::theme::current(context);
        div()
            .id("add-folder")
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .gap_1()
            .text_size(px(size::SMALL))
            .text_color(hsla(theme.text_muted))
            .hover(|style| style.bg(hsla(theme.hover_overlay)))
            .cursor_pointer()
            .child(icon(icons::PLUS))
            .child("Add folder")
            .on_mouse_down(
                MouseButton::Left,
                context.listener(|this, _, window, context| {
                    this.pick_folder(window, context);
                }),
            )
            .into_any_element()
    }
}

/// A folder's caret, open or not; a file's room for it.
fn caret(is_dir: bool, open: bool, theme: &Theme) -> gpui::Div {
    div()
        .w(px(12.))
        .flex_none()
        .text_color(hsla(theme.text_muted))
        .when(is_dir, |element| {
            element.child(icon(if open {
                icons::CARET_DOWN
            } else {
                icons::CARET_RIGHT
            }))
        })
}
