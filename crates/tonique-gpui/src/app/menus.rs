//! What the menu bar's menus hold, as the egui app's; its Settings button
//! is in the File menu here. Left out, as this app hasn't it: the
//! interface's zoom.

use gpui::Context;

use crate::{
    app::{
        PopupKind, Tonique, dialog::Dialog, menu_bar::BarMenu, project::ProjectAction,
        settings::SettingsTab,
    },
    config::keymap::Action,
    core::project::project_name,
    widgets::{Menu, MenuItem},
};

impl Tonique {
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
    fn submenu_open(&self, kind: PopupKind) -> bool {
        let submenu = self.popup.as_ref().and_then(|popup| popup.submenu);
        submenu.is_some_and(|submenu| submenu.kind == kind)
    }

    /// A menu item running `action`, with its shortcut.
    fn action_item(&self, text: &'static str, action: Action) -> MenuItem {
        MenuItem::new(text).shortcut(self.shortcut_text(action))
    }

    fn file_menu(&self, context: &mut Context<Self>) -> Menu {
        let project = |action: ProjectAction| {
            move |this: &mut Self, window: &mut gpui::Window, context: &mut Context<Self>| {
                this.project_action(action.clone(), window, context)
            }
        };
        let missing = self.state.missing_files().len();
        let missing_label = match missing {
            0 | 1 => "Missing files…".to_string(),
            count => format!("{count} missing files…"),
        };
        Menu::new(&self.theme)
            .item(
                self.action_item("New", Action::NewProject),
                context,
                project(ProjectAction::New),
            )
            .item(
                self.action_item("Open…", Action::OpenProject),
                context,
                project(ProjectAction::Open),
            )
            .submenu(
                MenuItem::new("Open recent"),
                PopupKind::RecentProjects,
                self.submenu_open(PopupKind::RecentProjects),
                context,
            )
            .separator()
            .item(
                self.action_item("Save", Action::SaveProject),
                context,
                project(ProjectAction::Save),
            )
            .item(
                self.action_item("Save as…", Action::SaveProjectAs),
                context,
                project(ProjectAction::SaveAs),
            )
            .item(
                self.action_item("Export audio…", Action::ExportAudio),
                context,
                |this, window, context| this.open_export(window, context),
            )
            .separator()
            .item(
                self.action_item("Settings…", Action::OpenSettings),
                context,
                |this, _, _| this.open_settings(SettingsTab::General),
            )
            .separator()
            // Always there, so it can be found; usable while files are
            // missing.
            .item(
                MenuItem::new(missing_label).enabled(missing > 0),
                context,
                |this, _, _| this.show_missing_files(None),
            )
    }

    /// Recent projects by name, with their folder on the right to tell
    /// apart projects named alike; those not found are shown, disabled.
    pub(super) fn recent_projects_menu(&self, context: &mut Context<Self>) -> gpui::Div {
        let recent = self.project.recent();
        let mut menu = Menu::new_submenu(&self.theme);
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
        let state = &self.state;
        Menu::new(&self.theme)
            .item(
                self.action_item("Undo", Action::Undo)
                    .enabled(state.can_undo()),
                context,
                |this, _, _| this.state.undo(),
            )
            .item(
                self.action_item("Redo", Action::Redo)
                    .enabled(state.can_redo()),
                context,
                |this, _, _| this.state.redo(),
            )
            .separator()
            .item(
                self.action_item("Add audio track", Action::AddTrack),
                context,
                |this, _, _| {
                    this.state.add_track_below_selection();
                },
            )
    }

    fn view_menu(&self, context: &mut Context<Self>) -> Menu {
        let graph = self.graph.is_open();
        let menu = Menu::new(&self.theme);
        // The audio graph is a development tool.
        let menu = if !Action::ToggleGraphView.is_available() {
            menu
        } else {
            menu.item(
                MenuItem::new("Timeline").checked(!graph).keep_open(),
                context,
                |this, _, context| {
                    if this.graph.is_open() {
                        this.toggle_graph(context);
                    }
                },
            )
            .item(
                self.action_item("Audio graph", Action::ToggleGraphView)
                    .checked(graph)
                    .keep_open(),
                context,
                |this, _, context| {
                    if !this.graph.is_open() {
                        this.toggle_graph(context);
                    }
                },
            )
            .separator()
        };
        menu.item(
            self.action_item("Browser", Action::ToggleBrowser)
                .checked(self.browser_open)
                .keep_open(),
            context,
            |this, _, _| this.browser_open = !this.browser_open,
        )
        .item(
            self.action_item("Track list", Action::ToggleTrackList)
                .checked(self.headers.is_open())
                .keep_open(),
            context,
            |this, _, _| this.headers.toggle_list(),
        )
        .item(
            self.action_item("Effects panel", Action::ToggleEffectsPanel)
                .checked(self.bottom.open)
                .keep_open(),
            context,
            |this, _, _| this.bottom.toggle(),
        )
    }

    fn help_menu(&self, context: &mut Context<Self>) -> Menu {
        Menu::new(&self.theme).item(MenuItem::new("About Tonique"), context, |this, _, _| {
            this.dialog = Some(Dialog::About)
        })
    }
}
