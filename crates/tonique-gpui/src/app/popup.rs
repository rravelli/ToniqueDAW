//! The window's menus, opened in the menus' layer (`crate::popup`): which
//! one each [`PopupKind`] is, and building it.

use gpui::{Anchor, Context, Pixels, Point};
use tonique_engine::edit::{ClipId, PluginId, TrackId};

use crate::{
    app::{Tonique, menu_bar::BarMenu, settings::AudioChoice},
    browser::ListKind,
    popup::{self, MenuBuilder},
};

/// Which menu.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PopupKind {
    /// The grid's settings: from the transport bar and the timeline.
    Grid,
    Clip(ClipId),
    Track(TrackId),
    /// An effect of the bottom panel's.
    Effect(TrackId, PluginId),
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
    pub fn belongs_to_menu_bar(self) -> bool {
        matches!(self, Self::Bar(_))
    }
}

impl Tonique {
    /// Builds `kind`'s menu, one of the window's, each frame.
    pub fn menu_builder(&self, kind: PopupKind, context: &Context<Self>) -> MenuBuilder {
        popup::builder(context.weak_entity(), move |this, _, context| {
            this.popup_menu(kind, context)
        })
    }

    /// Opens `kind`'s menu with its `anchor` corner at `at`; or, when it's
    /// open, closes it (its button pressed again).
    pub fn toggle_popup(
        &mut self,
        kind: PopupKind,
        at: Point<Pixels>,
        anchor: Anchor,
        context: &mut Context<Self>,
    ) {
        let build = self.menu_builder(kind, context);
        popup::toggle(kind, at, anchor, build, context);
    }

    fn popup_menu(&mut self, kind: PopupKind, context: &mut Context<Self>) -> Option<gpui::Div> {
        Some(match kind {
            PopupKind::Grid => self.grid_menu(context),
            PopupKind::Clip(id) => self.clip_menu(id, context)?,
            PopupKind::Track(id) => self.track_menu(id, context)?,
            PopupKind::Effect(track, plugin) => self.effect_menu(track, plugin, context)?,
            PopupKind::ClipRuler(id, at) => self.clip_ruler_popup(id, at, context)?,
            PopupKind::ClipWarpMode(id) => self.clip_warp_menu(id, context)?,
            PopupKind::ClipWindow(id) => self.clip_window_menu(id, context)?,
            // The browser's own.
            PopupKind::BrowserRow(..) | PopupKind::BrowserRowLabel(..) => return None,
            PopupKind::Setting(choice) => self.audio_choice_menu(choice, context),
            PopupKind::Theme => self.theme_menu(context),
            PopupKind::Bar(menu) => self.bar_menu(menu, context),
            PopupKind::RecentProjects => self.recent_projects_menu(context),
            PopupKind::Memory => self.memory_popup(context),
        })
    }
}
