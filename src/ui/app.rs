use crate::{
    audio::host::AudioHost,
    config::settings::Settings,
    core::state::{PlaybackState, ToniqueProjectState},
    ui::{
        panels::{
            bottom_panel::UIBottomPanel,
            central_panel::UICentralPanel,
            left_panel::UILeftPanel,
            menu_bar::{UIMenuBar, set_ui_scale},
            top_bar::UITopBar,
        },
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
        _cc: &eframe::CreationContext<'_>,
    ) -> Self {
        let mut state = ToniqueProjectState::new(engine);
        state.attach_audio(audio, settings);
        Self {
            state,
            menu_bar: UIMenuBar::new(),
            top_bar: UITopBar::new(),
            bottom_panel: UIBottomPanel::new(),
            left_panel: UILeftPanel::new(),
            central_panel: UICentralPanel::new(),
            setting_window: UISettingsWindow::new(),
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
