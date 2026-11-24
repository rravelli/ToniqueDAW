use crate::bench::Bench;
use crate::ui::keymap::KeymapExt;
use crate::ui::theme::ToColor;
use crate::ui::widget::arrow_keys::holds_arrows;
use crate::{
    config::keymap::Action,
    core::state::{ActionOutcome, ProjectState},
    ui::{
        commands::Commands,
        export::ExportWindow,
        panels::{
            bottom_panel::BottomPanel,
            central_panel::CentralPanel,
            left_panel::LeftPanel,
            menu_bar::{AppMenuBar, set_ui_scale},
            transport_bar::TransportBar,
        },
        project::{ProjectAction, ProjectManager},
        settings::SettingsWindow,
        theme::{ThemeExt, ThemeLibrary},
        workspace::{BottomView, MainView, Workspace},
    },
};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

pub struct ToniqueApp {
    state: ProjectState,
    /// `TONIQUE_BENCH`: steers the view and measures frames.
    bench: Option<Bench>,
    workspace: Workspace,
    commands: Commands,
    menu_bar: AppMenuBar,
    transport_bar: TransportBar,
    bottom_panel: BottomPanel,
    left_panel: LeftPanel,
    central_panel: CentralPanel,
    setting_window: SettingsWindow,
    project: ProjectManager,
    export_window: ExportWindow,
}

impl ToniqueApp {
    pub fn new(mut state: ProjectState, cc: &eframe::CreationContext<'_>) -> Self {
        let themes = ThemeLibrary::load();
        let (theme, theme_warnings) = themes.resolve(&state.settings().theme);
        state.set_track_palette(&project_palette(&theme.palette));
        theme.install(&cc.egui_ctx);
        let mut bench = Bench::from_env("egui");
        if let Some(bench) = &mut bench {
            bench.setup(&mut state);
        }
        Self {
            project: ProjectManager::new(&state),
            state,
            bench,
            workspace: Workspace::default(),
            commands: Commands::default(),
            menu_bar: AppMenuBar::new(),
            transport_bar: TransportBar::new(),
            bottom_panel: BottomPanel::new(),
            left_panel: LeftPanel::new(),
            central_panel: CentralPanel::new(),
            setting_window: SettingsWindow::new(themes, theme_warnings),
            export_window: ExportWindow::new(),
        }
    }
}

impl eframe::App for ToniqueApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.guard(|app| app.update_state(ctx));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.guard(|app| app.show(ui));
    }
}

impl ToniqueApp {
    /// Run `f`; if it panics, save the project to a new file before
    /// crashing, so unsaved work isn't lost.
    fn guard(&mut self, f: impl FnOnce(&mut Self)) {
        let Err(panic) = catch_unwind(AssertUnwindSafe(|| f(self))) else {
            return;
        };
        match catch_unwind(AssertUnwindSafe(|| self.project.save_recovery(&self.state))) {
            Ok(Ok(Some(path))) => eprintln!(
                "Tonique crashed; unsaved work was saved to {}",
                path.display()
            ),
            Ok(Ok(None)) => {}
            Ok(Err(e)) => eprintln!("Tonique crashed; unsaved work couldn't be saved: {e}"),
            Err(_) => eprintln!("Tonique crashed; unsaved work couldn't be saved"),
        }
        resume_unwind(panic);
    }

    fn update_state(&mut self, ctx: &egui::Context) {
        if let Some(bench) = &mut self.bench {
            bench.frame(&mut self.state);
        }
        // Update state
        self.state
            .set_monitor_graph(self.workspace.main_view == MainView::Graph);
        self.state.update();
        // Keep the saved scale in sync with egui's zoom shortcuts (Ctrl +/-/0)
        if (ctx.zoom_factor() - self.state.settings().ui_scale).abs() > f32::EPSILON {
            set_ui_scale(ctx, &mut self.state, ctx.zoom_factor());
        }
        ctx.request_repaint();
    }

    fn show(&mut self, ui: &mut egui::Ui) {
        self.shortcuts(ui);
        self.state
            .set_track_palette(&project_palette(&ui.app_theme().palette));
        let (state, workspace, commands) =
            (&mut self.state, &mut self.workspace, &mut self.commands);
        let recent = self
            .menu_bar
            .show(ui, state, workspace, commands, self.project.recent());
        self.transport_bar.show(ui, state, workspace, commands);
        self.bottom_panel.show(ui, state, workspace, commands);
        self.left_panel.show(ui, state, workspace, commands);
        self.central_panel.show(ui, state, workspace, commands);

        if let Some(action) = recent {
            self.project.request(action, &mut self.state);
        }
        for action in self.commands.take_all() {
            self.dispatch(action);
        }
        self.setting_window.show(ui, &mut self.state);
        self.project.ui(ui, &mut self.state);
        if self.project.take_replaced() {
            self.workspace.bottom_view = BottomView::Effects;
        }
        self.export_window.show(ui, &self.state);
    }

    /// Queue this frame's shortcuts.
    fn shortcuts(&mut self, ui: &egui::Ui) {
        // Keys typed into a widget (or recorded as a shortcut) aren't
        // commands; a value control takes only the arrows.
        let arrows_held = holds_arrows(ui.ctx());
        if (ui.memory(|m| m.focused().is_some()) && !arrows_held) || !ui.input(|i| i.focused) {
            return;
        }
        let keymap = &self.state.settings().keymap;
        for action in ui.input(|i| keymap.triggered(i, !arrows_held)) {
            self.commands.push(action);
        }
    }

    /// Run an action no panel took. Those acting on the timeline's
    /// selection ([`Action::is_timeline`]) do nothing while it's hidden.
    fn dispatch(&mut self, action: Action) {
        let state = &mut self.state;
        if action.is_timeline() || state.run_action(action) != ActionOutcome::Unhandled {
            return;
        }
        let workspace = &mut self.workspace;
        match action {
            Action::ToggleBrowser => workspace.left_panel_open = !workspace.left_panel_open,
            Action::ToggleEffectsPanel => {
                workspace.bottom_panel_open = !workspace.bottom_panel_open
            }
            Action::ToggleTrackList => workspace.track_list_open = !workspace.track_list_open,
            Action::ToggleGraphView => workspace.toggle_graph(),
            Action::OpenSettings => self.setting_window.toggle(state),
            Action::NewProject => self.project.request(ProjectAction::New, state),
            Action::OpenProject => self.project.request(ProjectAction::Open, state),
            Action::SaveProject => self.project.request(ProjectAction::Save, state),
            Action::SaveProjectAs => self.project.request(ProjectAction::SaveAs, state),
            Action::ExportAudio => self.export_window.open(&self.project),
            // Renaming in place is the track list's.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;

/// The theme's palette, as the project gives it to new tracks.
fn project_palette(palette: &[egui::Color32]) -> Vec<crate::utils::color::Color> {
    palette.iter().map(|&c| c.to_color()).collect()
}
