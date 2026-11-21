//! The menu bar at the top of the window, as the egui app's: File, Edit,
//! View and Help. A title pressed opens its menu below it; while one is
//! open, hovering another title opens that one instead.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gpui::{
    Anchor, Bounds, Context, MouseButton, Pixels, Stateful, canvas, div, point, prelude::*, px,
};

use crate::{
    app::{Popup, PopupKind, Tonique},
    theme::{Theme, hsla, size},
};

pub const MENU_BAR_HEIGHT: f32 = 24.;
const TITLE_HEIGHT: f32 = 20.;
/// Between a title and its menu.
const MENU_OFFSET: f32 = 2.;

/// A menu of the bar.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BarMenu {
    File,
    Edit,
    View,
    Help,
}

impl BarMenu {
    const ALL: [Self; 4] = [Self::File, Self::Edit, Self::View, Self::Help];

    fn name(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Edit => "Edit",
            Self::View => "View",
            Self::Help => "Help",
        }
    }

    /// Whether `popup` is this menu, or one opened from it.
    fn shows(self, popup: PopupKind) -> bool {
        match popup {
            PopupKind::Bar(menu) => menu == self,
            _ => false,
        }
    }
}

/// Where the titles were laid out, to open their menus below them.
#[derive(Default)]
pub struct MenuBarUi {
    titles: Rc<RefCell<HashMap<BarMenu, Bounds<Pixels>>>>,
}

impl Tonique {
    /// Opens `menu` below its title; pressed again, closes it.
    fn open_bar_menu(&mut self, menu: BarMenu, toggle: bool) {
        let Some(title) = self.menu_bar.titles.borrow().get(&menu).copied() else {
            return;
        };
        let at = point(title.origin.x, title.bottom() + px(MENU_OFFSET));
        if toggle {
            self.toggle_popup(PopupKind::Bar(menu), at, Anchor::TopLeft);
        } else {
            self.popup = Some(Popup {
                kind: PopupKind::Bar(menu),
                at,
                anchor: Anchor::TopLeft,
                submenu: None,
            });
        }
    }

    /// Whether a menu of the bar is open.
    fn bar_menu_open(&self) -> bool {
        let open = self.popup.as_ref().map(|popup| popup.kind);
        open.is_some_and(|open| BarMenu::ALL.iter().any(|menu| menu.shows(open)))
    }

    pub(super) fn menu_bar(&self, context: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let titles = BarMenu::ALL.map(|menu| self.bar_title(menu, theme, context));
        div()
            .id("menu-bar")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .h(px(MENU_BAR_HEIGHT))
            .px(px(4.))
            .bg(hsla(theme.bg_panel))
            .border_b_1()
            .border_color(hsla(theme.separator))
            .text_size(px(size::BODY))
            // Beside the titles: closes the open menu.
            .on_mouse_down(
                MouseButton::Left,
                context.listener(|this, _, _, context| {
                    if this.bar_menu_open() {
                        this.popup = None;
                        context.notify();
                    }
                }),
            )
            .children(titles)
    }

    fn bar_title(
        &self,
        menu: BarMenu,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let open = self
            .popup
            .as_ref()
            .is_some_and(|popup| menu.shows(popup.kind));
        let slot = self.menu_bar.titles.clone();
        div()
            .id(menu.name())
            .relative()
            .flex()
            .items_center()
            .h(px(TITLE_HEIGHT))
            .px_2()
            .rounded(px(2.))
            .when(open, |element| element.bg(hsla(theme.bg_control)))
            .hover(|style| style.bg(hsla(theme.hover_overlay)))
            .cursor_pointer()
            .child(menu.name())
            .child(
                canvas(
                    move |bounds, _, _| {
                        slot.borrow_mut().insert(menu, bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, _, _, context| {
                    context.stop_propagation();
                    this.open_bar_menu(menu, true);
                    context.notify();
                }),
            )
            .on_hover(context.listener(move |this, hovered: &bool, _, context| {
                let elsewhere = !this
                    .popup
                    .as_ref()
                    .is_some_and(|popup| menu.shows(popup.kind));
                if *hovered && this.bar_menu_open() && elsewhere {
                    this.open_bar_menu(menu, false);
                    context.notify();
                }
            }))
    }
}
