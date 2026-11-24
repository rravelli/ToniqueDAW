//! What the menu bar's menus hold, as the egui app's; its Settings button
//! is in the File menu here. Left out, as this app hasn't it: the
//! interface's zoom.

use crate::model::HoldsProject;
use gpui::{App, Context};

use crate::{
    app::{
        PopupKind, Workspace, dialog::Dialog, menu_bar::BarMenu, project::ProjectAction,
        settings::SettingsTab,
    },
    widgets::{Menu, MenuItem},
};
use tonique_core::{config::keymap::Action, project::project_name};

impl Workspace {
    pub(super) fn bar_menu(&self, menu: BarMenu, context: &mut Context<Self>) -> gpui::Div {
        match menu {
            BarMenu::File => self.file_menu(context),
            BarMenu::Edit => self.edit_menu(context),
            BarMenu::View => self.view_menu(context),
            BarMenu::Help => self.help_menu(context),
        }
        .into_element()
    }

    /// Whether `kind`'s menu is shown beside an item of the open menu.
    fn submenu_open(&self, kind: PopupKind, context: &App) -> bool {
        crate::popup::submenu_shown(context) == Some(kind)
    }

    /// A menu item running `action`, with its shortcut.
    fn action_item(&self, text: &'static str, action: Action, context: &App) -> MenuItem {
        MenuItem::new(text).shortcut(self.shortcut_text(action, context))
    }

    fn file_menu(&self, context: &mut Context<Self>) -> Menu {
        let missing = self.state(context).missing_files().len();
        let missing_label = match missing {
            0 | 1 => "Missing files…".to_string(),
            count => format!("{count} missing files…"),
        };
        Menu::new(&crate::theme::current(context))
            .command(
                self.action_item("New", Action::NewProject, context),
                Action::NewProject,
                context,
            )
            .command(
                self.action_item("Open…", Action::OpenProject, context),
                Action::OpenProject,
                context,
            )
            .submenu(
                MenuItem::new("Open recent"),
                PopupKind::RecentProjects,
                self.submenu_open(PopupKind::RecentProjects, context),
                self.menu_builder(PopupKind::RecentProjects, context),
            )
            .separator()
            .command(
                self.action_item("Save", Action::SaveProject, context),
                Action::SaveProject,
                context,
            )
            .command(
                self.action_item("Save as…", Action::SaveProjectAs, context),
                Action::SaveProjectAs,
                context,
            )
            .command(
                self.action_item("Export audio…", Action::ExportAudio, context),
                Action::ExportAudio,
                context,
            )
            .separator()
            .item(
                self.action_item("Settings…", Action::OpenSettings, context),
                context,
                |this, _, context| this.open_settings(SettingsTab::General, context),
            )
            .separator()
            // Always there, so it can be found; usable while files are
            // missing.
            .item(
                MenuItem::new(missing_label).enabled(missing > 0),
                context,
                |this, _, context| this.show_missing_files(None, context),
            )
    }

    /// Recent projects by name, with their folder on the right to tell
    /// apart projects named alike; those not found are shown, disabled.
    pub(super) fn recent_projects_menu(&self, context: &mut Context<Self>) -> gpui::Div {
        let recent = self.files.recent();
        let mut menu = Menu::new_submenu(&crate::theme::current(context));
        if recent.is_empty() {
            let none = MenuItem::new("No recent projects").enabled(false);
            return menu.item(none, context, |_, _, _| {}).into_element();
        }
        for path in recent {
            let folder = path
                .parent()
                .and_then(|parent| parent.file_name())
                .map_or(String::new(), |folder| {
                    folder.to_string_lossy().into_owned()
                });
            let item = MenuItem::new(project_name(path))
                .shortcut(folder)
                .enabled(path.exists());
            let path = path.clone();
            menu = menu.item(item, context, move |this, window, context| {
                this.project_action(ProjectAction::OpenRecent(path.clone()), window, context)
            });
        }
        menu.separator()
            .item(
                MenuItem::new("Clear list"),
                context,
                |this, window, context| {
                    this.project_action(ProjectAction::ClearRecent, window, context)
                },
            )
            .into_element()
    }

    fn edit_menu(&self, context: &mut Context<Self>) -> Menu {
        let state = self.state(context);
        let (can_undo, can_redo) = (state.can_undo(), state.can_redo());
        Menu::new(&crate::theme::current(context))
            .command(
                self.action_item("Undo", Action::Undo, context)
                    .enabled(can_undo),
                Action::Undo,
                context,
            )
            .command(
                self.action_item("Redo", Action::Redo, context)
                    .enabled(can_redo),
                Action::Redo,
                context,
            )
            .separator()
            .command(
                self.action_item("Add audio track", Action::AddTrack, context),
                Action::AddTrack,
                context,
            )
    }

    fn view_menu(&self, context: &mut Context<Self>) -> Menu {
        let graph = self.graph.read(context).is_open();
        let menu = Menu::new(&crate::theme::current(context));
        // The audio graph is a development tool.
        let menu = if !Action::ToggleGraphView.is_available() {
            menu
        } else {
            menu.item(
                MenuItem::new("Timeline").checked(!graph).keep_open(),
                context,
                |this, _, context| {
                    if this.graph.read(context).is_open() {
                        this.toggle_graph(context);
                    }
                },
            )
            .item(
                self.action_item("Audio graph", Action::ToggleGraphView, context)
                    .checked(graph)
                    .keep_open(),
                context,
                |this, _, context| {
                    if !this.graph.read(context).is_open() {
                        this.toggle_graph(context);
                    }
                },
            )
            .separator()
        };
        menu.command(
            self.action_item("Browser", Action::ToggleBrowser, context)
                .checked(self.browser_open)
                .keep_open(),
            Action::ToggleBrowser,
            context,
        )
        .command(
            self.action_item("Track list", Action::ToggleTrackList, context)
                .checked(self.arrangement.read(context).headers.is_open())
                .keep_open(),
            Action::ToggleTrackList,
            context,
        )
        .command(
            self.action_item("Effects panel", Action::ToggleEffectsPanel, context)
                .checked(self.bottom.read(context).shown.open)
                .keep_open(),
            Action::ToggleEffectsPanel,
            context,
        )
    }

    fn help_menu(&self, context: &mut Context<Self>) -> Menu {
        Menu::new(&crate::theme::current(context)).item(
            MenuItem::new("About Tonique"),
            context,
            |this, _, _| this.dialog = Some(Dialog::About),
        )
    }
}
