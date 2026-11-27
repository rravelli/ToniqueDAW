//! The open menu, over everything: a context menu where the pointer was
//! right-clicked, or a button's menu below it. A click anywhere else
//! closes it, and only closes it; but the menu bar stays usable while one
//! of its menus is open, to go from one to the next. An item of the menu
//! can show a submenu beside it.
//!
//! What's open is an app global, so any view opens a menu, with a
//! [`MenuBuilder`] building it again each frame from that view (it shows
//! the view as it is). The menus' layer is a view of its own drawing it:
//! drawn after the window's root, so a builder can update any view, the
//! root included, and read what's open.

use std::rc::Rc;

use gpui::{
    Anchor, App, Context, Entity, Global, Pixels, Point, WeakEntity, Window, anchored, deferred,
    div, point, prelude::*, px,
};

use crate::app::{PopupKind, menu_bar::MENU_BAR_HEIGHT};

/// Over dialogs, as a dialog's dropdowns open menus.
const BACKDROP_PRIORITY: usize = 2;
const MENU_PRIORITY: usize = 3;
const SUBMENU_PRIORITY: usize = 4;

/// Builds a menu from the view it belongs to; `None` when what it was for
/// is gone.
pub type MenuBuilder = Rc<dyn Fn(&mut Window, &mut App) -> Option<gpui::Div>>;

/// A builder calling `build` on `view`.
pub fn builder<V: 'static>(
    view: WeakEntity<V>,
    build: impl Fn(&mut V, &mut Window, &mut Context<V>) -> Option<gpui::Div> + 'static,
) -> MenuBuilder {
    Rc::new(move |window, context| {
        view.update(context, |view, context| build(view, window, context))
            .ok()
            .flatten()
    })
}

#[derive(Clone)]
pub struct Popup {
    pub kind: PopupKind,
    /// In the window.
    pub at: Point<Pixels>,
    /// Which of its corners is at `at`.
    pub anchor: Anchor,
    build: MenuBuilder,
    /// Shown beside one of its items.
    submenu: Option<Submenu>,
}

/// A menu shown beside the item of the open menu it belongs to.
#[derive(Clone)]
struct Submenu {
    kind: PopupKind,
    /// Its top left, in the window.
    at: Point<Pixels>,
    build: MenuBuilder,
}

/// The open menu, if any.
#[derive(Default)]
struct Popups {
    popup: Option<Popup>,
}

impl Global for Popups {}

/// The menus' layer, drawing the open menu.
pub struct PopupLayer;

/// Makes the menus' layer, for the window to show.
pub fn init(context: &mut App) -> Entity<PopupLayer> {
    context.set_global(Popups::default());
    context.new(|_| PopupLayer)
}

fn popups(context: &App) -> &Popups {
    context.global::<Popups>()
}

/// Changes what's open, and draws the windows again.
fn change(context: &mut App, change: impl FnOnce(&mut Option<Popup>)) {
    change(&mut context.global_mut::<Popups>().popup);
    context.refresh_windows();
}

/// Opens `kind`'s menu, built by `build`, with its `anchor` corner at `at`.
pub fn open(
    kind: PopupKind,
    at: Point<Pixels>,
    anchor: Anchor,
    build: MenuBuilder,
    context: &mut App,
) {
    change(context, |popup| {
        *popup = Some(Popup {
            kind,
            at,
            anchor,
            build,
            submenu: None,
        });
    });
}

/// Opens `kind`'s menu; or, when it's open, closes it (its button pressed
/// again).
pub fn toggle(
    kind: PopupKind,
    at: Point<Pixels>,
    anchor: Anchor,
    build: MenuBuilder,
    context: &mut App,
) {
    if shown(context) == Some(kind) {
        close(context);
    } else {
        open(kind, at, anchor, build, context);
    }
}

pub fn close(context: &mut App) {
    if shown(context).is_some() {
        change(context, |popup| *popup = None);
    }
}

/// Which menu is open, if any.
pub fn shown(context: &App) -> Option<PopupKind> {
    popups(context).popup.as_ref().map(|popup| popup.kind)
}

/// Shows `kind`'s menu beside an item of the open menu, at `at`.
pub fn open_submenu(kind: PopupKind, at: Point<Pixels>, build: MenuBuilder, context: &mut App) {
    change(context, |popup| {
        if let Some(popup) = popup.as_mut() {
            popup.submenu = Some(Submenu { kind, at, build });
        }
    });
}

pub fn close_submenu(context: &mut App) {
    if submenu_shown(context).is_some() {
        change(context, |popup| {
            if let Some(popup) = popup.as_mut() {
                popup.submenu = None;
            }
        });
    }
}

/// Which menu is shown beside an item of the open one, if any.
pub fn submenu_shown(context: &App) -> Option<PopupKind> {
    let popup = popups(context).popup.as_ref()?;
    popup.submenu.as_ref().map(|submenu| submenu.kind)
}

impl Render for PopupLayer {
    /// The open menu, and under it a backdrop over the window taking the
    /// click that closes it.
    fn render(&mut self, window: &mut Window, context: &mut Context<Self>) -> impl IntoElement {
        let Some(popup) = popups(context).popup.clone() else {
            return div().into_any_element();
        };
        let Some(menu) = (popup.build)(window, context) else {
            // What it was for is gone.
            close(context);
            return div().into_any_element();
        };
        let size = window.viewport_size();
        let top = if popup.kind.belongs_to_menu_bar() {
            px(MENU_BAR_HEIGHT)
        } else {
            px(0.)
        };
        let backdrop = div()
            .id("popup-backdrop")
            .w(size.width)
            .h(size.height - top)
            .occlude()
            .on_any_mouse_down(|_, _, context| {
                context.stop_propagation();
                close(context);
            });
        let menu = menu
            .id("popup")
            .occlude()
            // Its own clicks stay in it.
            .on_any_mouse_down(|_, _, context| context.stop_propagation());
        let submenu = popup.submenu.and_then(|submenu| {
            let menu = (submenu.build)(window, context)?;
            let menu = menu
                .id("submenu")
                .occlude()
                .on_any_mouse_down(|_, _, context| context.stop_propagation());
            let placed = anchored()
                .position(submenu.at)
                .snap_to_window_with_margin(px(4.))
                .child(menu);
            Some(deferred(placed).priority(SUBMENU_PRIORITY))
        });
        // Over the whole window, though it only shows what it defers: a view
        // painting nothing of its own still needs bounds to be painted.
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(
                deferred(anchored().position(point(px(0.), top)).child(backdrop))
                    .priority(BACKDROP_PRIORITY),
            )
            .child(
                deferred(
                    anchored()
                        .position(popup.at)
                        .anchor(popup.anchor)
                        .snap_to_window_with_margin(px(4.))
                        .child(menu),
                )
                .priority(MENU_PRIORITY),
            )
            .children(submenu)
            .into_any_element()
    }
}
