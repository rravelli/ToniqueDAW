//! A menu, as Tonique's context menus: labels, separators and items with
//! an icon (or a tick) before their text and their shortcut after it. An
//! item closes the menu it's in when pressed, unless it's a toggle that
//! stays open or it opened another menu in its place. An item can show a
//! submenu beside it.

use std::{cell::Cell, rc::Rc};

use gpui::{
    Bounds, Context, MouseButton, Pixels, SharedString, Stateful, Window, canvas, div, point,
    prelude::*, px,
};

use crate::{
    app::PopupKind,
    icons,
    popup::{self, MenuBuilder},
    theme::{Color, FONT_UI, Theme, hsla, icon, size},
};

const WIDTH: f32 = 120.;
const ITEM_HEIGHT: f32 = 16.;
/// The icon's column, from the item's left edge to its text.
const ICON_WIDTH: f32 = 15.;
/// Between an item's text and its shortcut.
const SHORTCUT_GAP: f32 = 12.;
/// Between the menu's edges and its items.
const MENU_PADDING: f32 = 4.;

/// An entry of a [`Menu`].
pub struct MenuItem {
    id: SharedString,
    text: SharedString,
    icon: Option<&'static str>,
    checked: bool,
    /// A dot of this colour before its text: a label's.
    swatch: Option<Color>,
    color: Option<Color>,
    keep_open: bool,
    shortcut: Option<SharedString>,
    enabled: bool,
}

impl MenuItem {
    pub fn new(text: impl Into<SharedString>) -> Self {
        let text = text.into();
        Self {
            id: text.clone(),
            text,
            icon: None,
            checked: false,
            swatch: None,
            color: None,
            keep_open: false,
            shortcut: None,
            enabled: true,
        }
    }
    /// Shown on the right, muted: a shortcut, or a hint like a folder.
    pub fn shortcut(self, shortcut: impl Into<SharedString>) -> Self {
        Self {
            shortcut: Some(shortcut.into()),
            ..self
        }
    }
    /// Disabled, it's dimmed and does nothing.
    pub fn enabled(self, enabled: bool) -> Self {
        Self { enabled, ..self }
    }
    pub fn icon(self, icon: &'static str) -> Self {
        Self {
            icon: Some(icon),
            ..self
        }
    }
    /// Ticked.
    pub fn checked(self, checked: bool) -> Self {
        Self { checked, ..self }
    }
    /// A dot of `color` before its text; ticked, the tick goes after it.
    pub fn swatch(self, color: Color) -> Self {
        Self {
            swatch: Some(color),
            ..self
        }
    }
    pub fn color(self, color: Color) -> Self {
        Self {
            color: Some(color),
            ..self
        }
    }
    /// Pressing it leaves the menu open.
    pub fn keep_open(self) -> Self {
        Self {
            keep_open: true,
            ..self
        }
    }
}

pub struct Menu {
    element: gpui::Div,
    theme: Theme,
    /// Shown beside an item of another menu.
    is_submenu: bool,
}

impl Menu {
    pub fn new(theme: &Theme) -> Self {
        let element = div()
            .flex()
            .flex_col()
            .min_w(px(WIDTH))
            .p(px(MENU_PADDING))
            .rounded(px(2.))
            .bg(hsla(theme.bg_raised))
            .border_1()
            .border_color(hsla(theme.border))
            .font_family(FONT_UI)
            .text_size(px(size::LABEL))
            .text_color(hsla(theme.text));
        Self {
            element,
            theme: theme.clone(),
            is_submenu: false,
        }
    }

    /// A menu shown beside an item of another one: its items leave it
    /// open when hovered.
    pub fn new_submenu(theme: &Theme) -> Self {
        Self {
            is_submenu: true,
            ..Self::new(theme)
        }
    }

    /// A heading, muted.
    pub fn label(self, text: impl Into<SharedString>) -> Self {
        let theme = &self.theme;
        let label = div()
            .px(px(3.))
            .pb(px(3.))
            .text_color(hsla(theme.text_muted))
            .whitespace_nowrap()
            .child(text.into());
        Self {
            element: self.element.child(label),
            ..self
        }
    }

    pub fn separator(self) -> Self {
        let line = div().my(px(2.)).h(px(1.)).bg(hsla(self.theme.separator));
        Self {
            element: self.element.child(line),
            ..self
        }
    }

    /// Anything else, as a row of its own.
    pub fn child(self, child: impl IntoElement) -> Self {
        Self {
            element: self.element.child(child),
            ..self
        }
    }

    /// An item running `action` when pressed. Hovered, it closes the
    /// submenu shown beside another item.
    pub fn item<V: 'static>(
        self,
        item: MenuItem,
        context: &mut Context<V>,
        action: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
    ) -> Self {
        let keep_open = item.keep_open;
        let enabled = item.enabled;
        let row = self.row(item, None);
        let row = if self.is_submenu {
            row
        } else {
            row.on_hover(|hovered: &bool, _, context| {
                if *hovered {
                    popup::close_submenu(context);
                }
            })
        };
        let row = if !enabled {
            row
        } else {
            row.cursor_pointer().on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, _, window, context| {
                    context.stop_propagation();
                    let shown = popup::shown(context);
                    action(this, window, context);
                    // Unless the action opened another menu in its place.
                    let replaced = popup::shown(context) != shown;
                    if !keep_open && !replaced {
                        popup::close(context);
                    }
                    context.notify();
                }),
            )
        };
        Self {
            element: self.element.child(row),
            ..self
        }
    }

    /// An item running the keymap's `action`, as its shortcut does.
    pub fn command<V: 'static>(
        self,
        item: MenuItem,
        action: tonique_core::config::keymap::Action,
        context: &mut Context<V>,
    ) -> Self {
        self.item(item, context, move |_, window, context| {
            crate::app::command::dispatch(action, window, context);
        })
    }

    /// An item showing `kind`'s menu, built by `build`, beside it while
    /// hovered or pressed; `open` while it does.
    pub fn submenu(self, item: MenuItem, kind: PopupKind, open: bool, build: MenuBuilder) -> Self {
        let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::default();
        let slot = bounds.clone();
        let open_beside = move |context: &mut gpui::App| {
            let item = bounds.get();
            // Its first item level with this one.
            let top = item.top() - px(MENU_PADDING + 1.);
            popup::open_submenu(kind, point(item.right(), top), build.clone(), context);
        };
        let row = self
            .row(item, Some(icons::CARET_RIGHT))
            .relative()
            .cursor_pointer()
            .when(open, |row| row.bg(hsla(self.theme.hover_overlay)))
            .child(
                canvas(move |bounds, _, _| slot.set(bounds), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .on_hover({
                let open_beside = open_beside.clone();
                move |hovered: &bool, _, context| {
                    if *hovered {
                        open_beside(context);
                    }
                }
            })
            .on_mouse_down(MouseButton::Left, move |_, _, context| {
                context.stop_propagation();
                open_beside(context);
            });
        Self {
            element: self.element.child(row),
            ..self
        }
    }

    /// An item's row: its mark, its text, and on the right its shortcut, or
    /// `trailing` (a submenu's arrow).
    fn row(&self, item: MenuItem, trailing: Option<&'static str>) -> Stateful<gpui::Div> {
        let theme = &self.theme;
        let mark = if item.checked && item.swatch.is_none() {
            Some(icons::CHECK)
        } else {
            item.icon
        };
        let trailing = trailing.or((item.checked && item.swatch.is_some()).then_some(icons::CHECK));
        let (text_color, mark_color) = if item.enabled {
            (
                item.color.unwrap_or(theme.text),
                item.color.unwrap_or(theme.text_muted),
            )
        } else {
            (theme.text_disabled, theme.text_disabled)
        };
        let muted = |element: gpui::Div| {
            element
                .flex_none()
                .ml(px(SHORTCUT_GAP))
                .whitespace_nowrap()
                .text_size(px(size::SMALL))
                .text_color(hsla(theme.text_disabled))
        };
        let row = div()
            .id(item.id)
            .flex()
            .items_center()
            .h(px(ITEM_HEIGHT))
            .pl(px(3.))
            .pr(px(4.))
            .rounded(px(2.))
            .text_color(hsla(text_color))
            .child(
                div()
                    .w(px(ICON_WIDTH))
                    .flex_none()
                    .text_color(hsla(mark_color))
                    .children(mark.map(icon))
                    .children(
                        item.swatch
                            .map(|color| div().size(px(8.)).rounded_full().bg(hsla(color))),
                    ),
            )
            .child(div().flex_1().whitespace_nowrap().child(item.text))
            .children(item.shortcut.map(|shortcut| muted(div()).child(shortcut)))
            .children(trailing.map(|glyph| muted(icon(glyph))));
        if item.enabled {
            row.hover(|style| style.bg(hsla(theme.hover_overlay)))
        } else {
            row
        }
    }

    pub fn into_element(self) -> gpui::Div {
        self.element
    }
}
