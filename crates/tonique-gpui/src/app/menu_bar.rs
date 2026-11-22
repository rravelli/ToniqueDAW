//! The menu bar at the top of the window, as the egui app's: File, Edit,
//! View and Help. A title pressed opens its menu below it; while one is
//! open, hovering another title opens that one instead. In debug builds,
//! the interface's frame rate and memory on the right; clicked, they open
//! the memory's details.

use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Instant};

use gpui::{
    Anchor, Bounds, Context, MouseButton, Pixels, Stateful, canvas, div, point, prelude::*, px,
};

use crate::{
    app::{Popup, PopupKind, Tonique, memory},
    theme::{Theme, hsla, size},
};

pub const MENU_BAR_HEIGHT: f32 = 24.;
const TITLE_HEIGHT: f32 = 20.;
/// Between a title and its menu.
const MENU_OFFSET: f32 = 2.;
/// How much of a new frame's time goes into the shown average.
const FRAME_SMOOTHING: f64 = 0.1;
/// Longer between two frames: the window was idle, not slow.
const IDLE_SECONDS: f64 = 0.5;
/// How often the process's memory is read again, in seconds.
const MEMORY_SECONDS: f64 = 1.;

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
    last_frame: Option<Instant>,
    /// Smoothed time between frames, in seconds.
    frame_seconds: Option<f64>,
    /// The process's memory, and when it was read.
    resident: Option<(usize, Instant)>,
    /// Where the frame rate and memory were laid out, to open the
    /// memory's details below them.
    debug_bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
}

impl MenuBarUi {
    /// Call once per frame, to time it.
    pub fn frame(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_frame.replace(now) {
            let elapsed = (now - last).as_secs_f64();
            if elapsed < IDLE_SECONDS {
                self.frame_seconds = Some(match self.frame_seconds {
                    Some(average) => average + (elapsed - average) * FRAME_SMOOTHING,
                    None => elapsed,
                });
            }
        }
        if cfg!(debug_assertions) {
            let stale = self
                .resident
                .is_none_or(|(_, read)| (now - read).as_secs_f64() >= MEMORY_SECONDS);
            if stale {
                self.resident = memory::resident_bytes().map(|bytes| (bytes, now));
            }
        }
    }

    /// The process's memory, as last read.
    pub fn resident_bytes(&self) -> Option<usize> {
        self.resident.map(|(bytes, _)| bytes)
    }

    fn debug_text(&self) -> Option<String> {
        let seconds = self.frame_seconds?.max(1e-5);
        let mut text = format!("{:.0} fps · {:.1} ms", 1. / seconds, seconds * 1000.);
        if let Some(bytes) = self.resident_bytes() {
            text.push_str(" · ");
            text.push_str(&memory::format_bytes(bytes));
        }
        Some(text)
    }
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
        let theme = &*crate::theme::current(context);
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
            .child(div().flex_1())
            .when(cfg!(debug_assertions), |element| {
                element.children(self.debug_info(context))
            })
    }

    /// The frame rate and memory; clicked, the memory's details.
    fn debug_info(&self, context: &mut Context<Self>) -> Option<Stateful<gpui::Div>> {
        let theme = &*crate::theme::current(context);
        let text = self.menu_bar.debug_text()?;
        let open = self
            .popup
            .as_ref()
            .is_some_and(|popup| popup.kind == PopupKind::Memory);
        let slot = self.menu_bar.debug_bounds.clone();
        Some(
            div()
                .id("debug-info")
                .relative()
                .flex_none()
                .flex()
                .items_center()
                .h(px(TITLE_HEIGHT))
                .px_2()
                .rounded(px(2.))
                .text_size(px(size::SMALL))
                .text_color(hsla(theme.text_muted))
                .when(open, |element| element.bg(hsla(theme.bg_control)))
                .hover(|style| style.bg(hsla(theme.hover_overlay)))
                .cursor_pointer()
                .child(text)
                .child(
                    canvas(
                        move |bounds, _, _| {
                            *slot.borrow_mut() = Some(bounds);
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    context.listener(|this, _, _, context| {
                        context.stop_propagation();
                        let Some(bounds) = *this.menu_bar.debug_bounds.borrow() else {
                            return;
                        };
                        let at = point(bounds.right(), bounds.bottom() + px(MENU_OFFSET));
                        this.toggle_popup(PopupKind::Memory, at, Anchor::TopRight);
                        context.notify();
                    }),
                ),
        )
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
