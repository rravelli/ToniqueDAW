//! The open menu, over everything: a context menu where the pointer was
//! right-clicked, or a button's menu below it. A click anywhere else
//! closes it, and only closes it; but the menu bar stays usable while one
//! of its menus is open, to go from one to the next. An item of the menu
//! can show a submenu beside it.

use gpui::{
    Anchor, AnyElement, Context, Pixels, Point, Window, anchored, deferred, div, point, prelude::*,
    px,
};
use tonique_engine::edit::{ClipId, PluginId, TrackId};

use crate::app::{
    Tonique,
    menu_bar::{BarMenu, MENU_BAR_HEIGHT},
    settings::AudioChoice,
};
use crate::browser::ListKind;

/// Over dialogs, as a dialog's dropdowns open menus.
const BACKDROP_PRIORITY: usize = 2;
const MENU_PRIORITY: usize = 3;
const SUBMENU_PRIORITY: usize = 4;

/// Which menu.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PopupKind {
    /// The grid's settings: from the transport bar and the timeline.
    Grid,
    Clip(ClipId),
    Track(TrackId),
    /// An effect of the bottom panel's.
    Effect(TrackId, PluginId),
    /// A filter's modes.
    FilterShape(TrackId, PluginId),
    /// An EQ band's shapes, from its picker.
    EqShape(TrackId, PluginId, usize),
    /// An EQ band's point, right-clicked.
    EqPoint(TrackId, PluginId, usize),
    /// The EQ's bands, to pick one.
    EqBand(TrackId, PluginId),
    /// Where to put bar 1 of a clip's grid, right-clicked this many
    /// seconds into its file.
    ClipRuler(ClipId, f64),
    ClipWarpMode(ClipId),
    /// A clip's stretch window.
    ClipWindow(ClipId),
    /// A row of one of the browser's lists.
    BrowserRow(ListKind, usize),
    /// The labels to give a row of the browser's, beside its menu.
    BrowserRowLabel(ListKind, usize),
    /// A settings dropdown's options.
    Setting(AudioChoice),
    /// The themes to choose from, in the settings.
    Theme,
    /// One of the menu bar's.
    Bar(BarMenu),
    RecentProjects,
    /// In debug builds, what memory the app takes.
    Memory,
}

impl PopupKind {
    /// Whether it belongs to the menu bar, which then stays usable.
    fn belongs_to_menu_bar(self) -> bool {
        matches!(self, Self::Bar(_))
    }
}

#[derive(Clone, Copy)]
pub struct Popup {
    pub kind: PopupKind,
    /// In the window.
    pub at: Point<Pixels>,
    /// Which of its corners is at `at`.
    pub anchor: Anchor,
    /// Shown beside one of its items.
    pub submenu: Option<Submenu>,
}

/// A menu shown beside the item of the open menu it belongs to.
#[derive(Clone, Copy, PartialEq)]
pub struct Submenu {
    pub kind: PopupKind,
    /// Its top left, in the window.
    pub at: Point<Pixels>,
}

impl Tonique {
    /// Opens `kind`'s menu with its top left at `at`; or, when it's open,
    /// closes it (its button pressed again).
    pub fn toggle_popup(&mut self, kind: PopupKind, at: Point<Pixels>, anchor: Anchor) {
        if self.popup.as_ref().is_some_and(|popup| popup.kind == kind) {
            self.popup = None;
        } else {
            self.popup = Some(Popup {
                kind,
                at,
                anchor,
                submenu: None,
            });
        }
    }

    /// Shows `kind`'s menu beside an item of the open menu, at `at`.
    pub fn open_submenu(&mut self, kind: PopupKind, at: Point<Pixels>) {
        if let Some(popup) = self.popup.as_mut() {
            popup.submenu = Some(Submenu { kind, at });
        }
    }

    pub fn close_submenu(&mut self) {
        if let Some(popup) = self.popup.as_mut() {
            popup.submenu = None;
        }
    }

    fn popup_menu(&mut self, kind: PopupKind, context: &mut Context<Self>) -> Option<gpui::Div> {
        Some(match kind {
            PopupKind::Grid => self.grid_menu(context),
            PopupKind::Clip(id) => self.clip_menu(id, context)?,
            PopupKind::Track(id) => self.track_menu(id, context)?,
            PopupKind::Effect(track, plugin) => self.effect_menu(track, plugin, context)?,
            PopupKind::FilterShape(track, plugin) => {
                self.filter_shape_menu(track, plugin, context)?
            }
            PopupKind::EqShape(track, plugin, band) => {
                self.eq_shape_menu(track, plugin, band, false, context)?
            }
            PopupKind::EqPoint(track, plugin, band) => {
                self.eq_shape_menu(track, plugin, band, true, context)?
            }
            PopupKind::EqBand(_, plugin) => self.eq_band_menu(plugin, context)?,
            PopupKind::ClipRuler(id, at) => self.clip_ruler_popup(id, at, context)?,
            PopupKind::ClipWarpMode(id) => self.clip_warp_menu(id, context)?,
            PopupKind::ClipWindow(id) => self.clip_window_menu(id, context)?,
            PopupKind::BrowserRow(list, index) => self.browser_row_menu(list, index, context)?,
            PopupKind::BrowserRowLabel(list, index) => {
                self.browser_label_menu(list, index, context)?
            }
            PopupKind::Setting(choice) => self.audio_choice_menu(choice, context),
            PopupKind::Theme => self.theme_menu(context),
            PopupKind::Bar(menu) => self.bar_menu(menu, context),
            PopupKind::RecentProjects => self.recent_projects_menu(context),
            PopupKind::Memory => self.memory_popup(context),
        })
    }

    /// The open menu, and under it a backdrop over the window taking the
    /// click that closes it.
    pub(super) fn popup_view(
        &mut self,
        window: &Window,
        context: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(popup) = &self.popup else {
            return Vec::new();
        };
        let (kind, at, anchor, submenu) = (popup.kind, popup.at, popup.anchor, popup.submenu);
        let Some(menu) = self.popup_menu(kind, context) else {
            // What it was for is gone.
            self.popup = None;
            return Vec::new();
        };
        let size = window.viewport_size();
        let top = if kind.belongs_to_menu_bar() {
            px(MENU_BAR_HEIGHT)
        } else {
            px(0.)
        };
        let backdrop = div()
            .id("popup-backdrop")
            .w(size.width)
            .h(size.height - top)
            .occlude()
            .on_any_mouse_down(context.listener(|this, _, _, context| {
                context.stop_propagation();
                this.popup = None;
                context.notify();
            }));
        let menu = menu
            .id("popup")
            .occlude()
            // Its own clicks stay in it.
            .on_any_mouse_down(|_, _, context| context.stop_propagation());
        let submenu = submenu.and_then(|submenu| {
            let menu = self.popup_menu(submenu.kind, context)?;
            let menu = menu
                .id("submenu")
                .occlude()
                .on_any_mouse_down(|_, _, context| context.stop_propagation());
            let placed = anchored()
                .position(submenu.at)
                .snap_to_window_with_margin(px(4.))
                .child(menu);
            Some(
                deferred(placed)
                    .priority(SUBMENU_PRIORITY)
                    .into_any_element(),
            )
        });
        let mut layers = vec![
            deferred(anchored().position(point(px(0.), top)).child(backdrop))
                .priority(BACKDROP_PRIORITY)
                .into_any_element(),
            deferred(
                anchored()
                    .position(at)
                    .anchor(anchor)
                    .snap_to_window_with_margin(px(4.))
                    .child(menu),
            )
            .priority(MENU_PRIORITY)
            .into_any_element(),
        ];
        layers.extend(submenu);
        layers
    }
}
