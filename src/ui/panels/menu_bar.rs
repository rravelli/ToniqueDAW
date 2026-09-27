use crate::{
    config::{keymap::Action, settings::UI_SCALE_RANGE},
    core::state::{CentralView, ToniqueProjectState},
    ui::theme::ThemeExt,
};
use egui::{Button, Context, Frame, Margin, MenuBar, Ui};

const ZOOM_STEP: f32 = 0.1;

/// Application menus at the very top of the window.
pub struct UIMenuBar;

/// What the menu bar asks the app to do.
#[derive(Default)]
pub struct MenuActions {
    pub open_settings: bool,
}

impl UIMenuBar {
    pub fn new() -> Self {
        Self
    }

    pub fn show(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) -> MenuActions {
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
