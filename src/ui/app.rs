use crate::{
    audio::host::AudioHost,
    config::{keymap::Action, settings::Settings},
    core::state::{CentralView, PlaybackState, ToniqueProjectState},
    ui::{
        panels::{
            bottom_panel::UIBottomPanel,
            central_panel::UICentralPanel,
            left_panel::UILeftPanel,
            menu_bar::{UIMenuBar, set_ui_scale},
            top_bar::UITopBar,
        },
        theme::{ThemeExt, ThemeLibrary},
        windows::settings::UISettingsWindow,
    },
};
use tonique_engine::engine::Engine;

pub struct ToniqueApp {
    state: ToniqueProjectState,
    menu_bar: UIMenuBar,
    top_bar: UITopBar,
    bottom_panel: UIBottomPanel,
    left_panel: UILeftPanel,
    central_panel: UICentralPanel,
    setting_window: UISettingsWindow,
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
        theme.install(&cc.egui_ctx);
        let mut state = ToniqueProjectState::new(engine);
        state.attach_audio(audio, settings);
        Self {
            state,
            menu_bar: UIMenuBar::new(),
            top_bar: UITopBar::new(),
            bottom_panel: UIBottomPanel::new(),
            left_panel: UILeftPanel::new(),
            central_panel: UICentralPanel::new(),
            setting_window: UISettingsWindow::new(themes, theme_warnings),
        }
    }
}

impl eframe::App for ToniqueApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Update state
        self.state.update();
        // Keep the saved scale in sync with egui's zoom shortcuts (Ctrl +/-/0)
        if (ctx.zoom_factor() - self.state.settings().ui_scale).abs() > f32::EPSILON {
            set_ui_scale(ctx, &mut self.state, ctx.zoom_factor());
        }
        ctx.request_repaint();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_shortcuts(ui);
        self.state.set_track_palette(&ui.app_theme().palette);
        let actions = self.menu_bar.show(ui, &mut self.state);
        self.top_bar.show(ui, &mut self.state);
        self.bottom_panel.show(ui, &mut self.state);
        self.left_panel.show(ui, &mut self.state);
        self.central_panel.show(ui, &mut self.state);

        if actions.open_settings {
            self.setting_window.open(&self.state);
        }
        self.setting_window.show(ui, &mut self.state);
    }
}

impl ToniqueApp {
    /// Shortcuts that work in every view. The timeline handles the ones
    /// acting on its selection ([`Action::is_timeline`]).
    fn handle_shortcuts(&mut self, ui: &egui::Ui) {
        // Keys typed into a widget (or recorded as a shortcut) aren't commands.
        if ui.memory(|m| m.focused().is_some()) || !ui.input(|i| i.focused) {
            return;
        }
        let actions = ui.input(|i| self.state.settings().keymap.triggered(i));
        let state = &mut self.state;
        for action in actions.into_iter().filter(|a| !a.is_timeline()) {
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
                Action::ToggleBrowser => state.left_panel_open = !state.left_panel_open,
                Action::ToggleEffectsPanel => state.bottom_panel_open = !state.bottom_panel_open,
                Action::ToggleGraphView => {
                    state.central_view = match state.central_view {
                        CentralView::Graph => CentralView::Timeline,
                        _ => CentralView::Graph,
                    }
                }
                Action::OpenSettings => self.setting_window.toggle(state),
                _ => {}
            }
        }
    }
}
