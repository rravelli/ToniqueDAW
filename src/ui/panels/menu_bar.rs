use crate::{
    config::{keymap::Action, settings::UI_SCALE_RANGE},
    core::project::project_name,
    core::state::{CentralView, ToniqueProjectState},
    ui::{project::ProjectAction, theme::ThemeExt},
};
use egui::{Button, Context, Frame, Margin, MenuBar, Ui};
use std::path::PathBuf;

const ZOOM_STEP: f32 = 0.1;

/// Application menus at the very top of the window.
pub struct UIMenuBar;

/// What the menu bar asks the app to do.
#[derive(Default)]
pub struct MenuActions {
    pub open_settings: bool,
    pub project: Option<ProjectAction>,
}

impl UIMenuBar {
    pub fn new() -> Self {
        Self
    }

    /// `recent`: recently opened projects, newest first.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        state: &mut ToniqueProjectState,
        recent: &[PathBuf],
    ) -> MenuActions {
        let mut actions = MenuActions::default();
        egui::Panel::top("menu-bar")
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(ui.app_theme().bg_panel)
                    .inner_margin(Margin::symmetric(4, 2)),
            )
            .show(ui, |ui| {
                MenuBar::new().ui(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 8.;
                    actions.project = self.file_menu(ui, state, recent);
                    self.edit_menu(ui, state);
                    self.view_menu(ui, state);
                    let tooltip = state.settings().keymap.with_shortcut(
                        ui.ctx(),
                        "Settings",
                        Action::OpenSettings,
                    );
                    if ui.button("Settings").on_hover_text(tooltip).clicked() {
                        actions.open_settings = true;
                    }
                });
            });
        actions
    }

    fn file_menu(
        &self,
        ui: &mut Ui,
        state: &ToniqueProjectState,
        recent: &[PathBuf],
    ) -> Option<ProjectAction> {
        let mut picked = None;
        ui.menu_button("File", |ui| {
            let keymap = &state.settings().keymap;
            let shortcut = |action| keymap.shortcut_text(ui.ctx(), action);
            let (new, open, save, save_as) = (
                shortcut(Action::NewProject),
                shortcut(Action::OpenProject),
                shortcut(Action::SaveProject),
                shortcut(Action::SaveProjectAs),
            );
            if menu_item(ui, "New", new) {
                picked = Some(ProjectAction::New);
            }
            if menu_item(ui, "Open…", open) {
                picked = Some(ProjectAction::Open);
            }
            ui.menu_button("Open recent", |ui| {
                if let Some(project) = recent_menu(ui, recent) {
                    picked = Some(project);
                }
            });
            ui.separator();
            if menu_item(ui, "Save", save) {
                picked = Some(ProjectAction::Save);
            }
            if menu_item(ui, "Save as…", save_as) {
                picked = Some(ProjectAction::SaveAs);
            }
        });
        picked
    }

    fn edit_menu(&self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        ui.menu_button("Edit", |ui| {
            let keymap = &state.settings().keymap;
            let undo =
                Button::new("Undo").shortcut_text(keymap.shortcut_text(ui.ctx(), Action::Undo));
            let redo =
                Button::new("Redo").shortcut_text(keymap.shortcut_text(ui.ctx(), Action::Redo));
            if ui.add_enabled(state.can_undo(), undo).clicked() {
                state.undo();
            }
            if ui.add_enabled(state.can_redo(), redo).clicked() {
                state.redo();
            }
            ui.separator();
            let add_track = Button::new("Add audio track").shortcut_text(
                state
                    .settings()
                    .keymap
                    .shortcut_text(ui.ctx(), Action::AddTrack),
            );
            if ui.add(add_track).clicked() {
                state.add_track();
            }
        });
    }

    fn view_menu(&self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        ui.menu_button("View", |ui| {
            let keymap = state.settings().keymap.clone();
            let label = |text, action| keymap.with_shortcut(ui.ctx(), text, action);
            let graph = label("Audio graph", Action::ToggleGraphView);
            let browser = label("Browser", Action::ToggleBrowser);
            let effects = label("Effects panel", Action::ToggleEffectsPanel);
            ui.radio_value(&mut state.central_view, CentralView::Timeline, "Timeline");
            ui.radio_value(&mut state.central_view, CentralView::Graph, graph);
            ui.separator();
            ui.checkbox(&mut state.left_panel_open, browser);
            ui.checkbox(&mut state.bottom_panel_open, effects);
            ui.separator();
            let scale = state.settings().ui_scale;
            if ui
                .add(Button::new("Zoom in").shortcut_text("Ctrl++"))
                .clicked()
            {
                set_ui_scale(ui.ctx(), state, scale + ZOOM_STEP);
            }
            if ui
                .add(Button::new("Zoom out").shortcut_text("Ctrl+-"))
                .clicked()
            {
                set_ui_scale(ui.ctx(), state, scale - ZOOM_STEP);
            }
            if ui
                .add(Button::new("Reset zoom").shortcut_text("Ctrl+0"))
                .clicked()
            {
                set_ui_scale(ui.ctx(), state, 1.);
            }
        });
    }
}

/// Apply and save a new interface scale.
pub fn set_ui_scale(ctx: &Context, state: &mut ToniqueProjectState, scale: f32) {
    let scale = scale.clamp(*UI_SCALE_RANGE.start(), *UI_SCALE_RANGE.end());
    ctx.set_zoom_factor(scale);
    let mut settings = state.settings().clone();
    settings.ui_scale = scale;
    state.apply_settings(settings);
}

fn menu_item(ui: &mut Ui, label: &str, shortcut: String) -> bool {
    ui.add(Button::new(label).shortcut_text(shortcut)).clicked()
}

/// Recent projects by name, with their folder on the right to tell apart
/// projects named alike. Missing ones are shown but disabled.
fn recent_menu(ui: &mut Ui, recent: &[PathBuf]) -> Option<ProjectAction> {
    if recent.is_empty() {
        ui.add_enabled(false, Button::new("No recent projects"));
        return None;
    }
    let mut picked = None;
    for path in recent {
        let folder = path
            .parent()
            .and_then(|p| p.file_name())
            .map_or(String::new(), |f| f.to_string_lossy().into_owned());
        let exists = path.exists();
        let button = Button::new(project_name(path)).shortcut_text(folder);
        let response = ui.add_enabled(exists, button);
        let hint = path.display().to_string();
        let response = if exists {
            response.on_hover_text(hint)
        } else {
            response.on_disabled_hover_text(format!("{hint} (not found)"))
        };
        if response.clicked() {
            picked = Some(ProjectAction::OpenRecent(path.clone()));
        }
    }
    ui.separator();
    if ui.button("Clear list").clicked() {
        picked = Some(ProjectAction::ClearRecent);
    }
    picked
}
