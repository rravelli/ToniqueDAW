use crate::{
    audio::host::AudioHost,
    config::{keymap::Action, settings::Settings},
    core::state::{PlaybackState, ProjectState},
    ui::{
        commands::Commands,
        panels::{
            bottom_panel::BottomPanel,
            central_panel::CentralPanel,
            left_panel::LeftPanel,
            menu_bar::{AppMenuBar, set_ui_scale},
            top_bar::TopBar,
        },
        project::{ProjectAction, ProjectManager},
        theme::{ThemeExt, ThemeLibrary},
        windows::settings::SettingsWindow,
        workspace::{MainView, Workspace},
    },
};
use tonique_engine::engine::Engine;

pub struct ToniqueApp {
    state: ProjectState,
    workspace: Workspace,
    commands: Commands,
    menu_bar: AppMenuBar,
    top_bar: TopBar,
    bottom_panel: BottomPanel,
    left_panel: LeftPanel,
    central_panel: CentralPanel,
    setting_window: SettingsWindow,
    project: ProjectManager,
}

impl ToniqueApp {
    pub fn new(
        engine: Engine,
        audio: AudioHost,
        settings: Settings,
        cc: &eframe::CreationContext<'_>,
    ) -> Self {
        let themes = ThemeLibrary::load();
        let (theme, theme_warnings) = themes.resolve(&settings.theme);
        let palette = theme.palette.clone();
        theme.install(&cc.egui_ctx);
        let mut state = ProjectState::new(engine);
        state.set_track_palette(&palette);
        state.attach_audio(audio, settings);
        Self {
            project: ProjectManager::new(&state),
            state,
            workspace: Workspace::default(),
            commands: Commands::default(),
            menu_bar: AppMenuBar::new(),
            top_bar: TopBar::new(),
            bottom_panel: BottomPanel::new(),
            left_panel: LeftPanel::new(),
            central_panel: CentralPanel::new(),
            setting_window: SettingsWindow::new(themes, theme_warnings),
        }
    }
}

impl eframe::App for ToniqueApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.shortcuts(ui);
        self.state.set_track_palette(&ui.app_theme().palette);
        let (state, workspace, commands) =
            (&mut self.state, &mut self.workspace, &mut self.commands);
        let recent = self
            .menu_bar
            .show(ui, state, workspace, commands, self.project.recent());
        self.top_bar.show(ui, state, workspace, commands);
        self.bottom_panel.show(ui, state, workspace, commands);
        self.left_panel.show(ui, state, workspace);
        self.central_panel.show(ui, state, workspace, commands);

        if let Some(action) = recent {
            self.project.request(action, &mut self.state);
        }
        for action in self.commands.take_all() {
            self.dispatch(action);
        }
        self.setting_window.show(ui, &mut self.state);
        self.project.ui(ui, &mut self.state);
    }
}

impl ToniqueApp {
    /// Queue this frame's shortcuts.
    fn shortcuts(&mut self, ui: &egui::Ui) {
        // Keys typed into a widget (or recorded as a shortcut) aren't commands.
        if ui.memory(|m| m.focused().is_some()) || !ui.input(|i| i.focused) {
            return;
        }
        for action in ui.input(|i| self.state.settings().keymap.triggered(i)) {
            self.commands.push(action);
        }
    }

    /// Run an action no panel took. Those acting on the timeline's
    /// selection ([`Action::is_timeline`]) do nothing while it's hidden.
    fn dispatch(&mut self, action: Action) {
        let state = &mut self.state;
        let workspace = &mut self.workspace;
        match action {
            Action::PlayStop => {
                if state.playback_state() == PlaybackState::Playing {
                    state.stop();
                } else {
                    state.play();
                }
            }
            // Loop the selection, or toggle looping
            Action::Loop => {
                if !state.loop_selection() {
                    state.set_looping(!state.looping());
                }
            }
            Action::ToggleMetronome => state.toggle_metronome(),
            Action::ToggleFollowPlayhead => state.set_follow_playhead(!state.follow_playhead()),
            Action::Undo => state.undo(),
            Action::Redo => state.redo(),
            Action::AddTrack => {
                state.add_track();
            }
            Action::GroupTracks => {
                let selected = state.selected_tracks().clone();
                state.group(&selected);
            }
            Action::ToggleBrowser => workspace.left_panel_open = !workspace.left_panel_open,
            Action::ToggleEffectsPanel => {
                workspace.bottom_panel_open = !workspace.bottom_panel_open
            }
            Action::ToggleGraphView => workspace.toggle_graph(),
            Action::OpenSettings => self.setting_window.toggle(state),
            Action::NewProject => self.project.request(ProjectAction::New, state),
            Action::OpenProject => self.project.request(ProjectAction::Open, state),
            Action::SaveProject => self.project.request(ProjectAction::Save, state),
            Action::SaveProjectAs => self.project.request(ProjectAction::SaveAs, state),
            Action::SelectAll
            | Action::Duplicate
            | Action::Delete
            | Action::SplitAtCursor
            | Action::NudgeLeft
            | Action::NudgeRight
            | Action::MoveTrackUp
            | Action::MoveTrackDown => {}
        }
    }
}
