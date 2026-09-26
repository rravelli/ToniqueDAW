use crate::{
    audio::host::PreviewLink,
    core::state::{PlaybackState, ToniqueProjectState},
    ui::panels::{
        bottom_panel::UIBottomPanel, central_panel::UICentralPanel, left_panel::UILeftPanel,
        top_bar::UITopBar,
    },
};
use tonique_engine::engine::Engine;

pub struct ToniqueApp {
    state: ToniqueProjectState,
    top_bar: UITopBar,
    bottom_panel: UIBottomPanel,
    left_panel: UILeftPanel,
    central_panel: UICentralPanel,
}

impl ToniqueApp {
    pub fn new(engine: Engine, preview: PreviewLink, _cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            state: ToniqueProjectState::new(engine, Some(preview)),
            top_bar: UITopBar::new(),
            bottom_panel: UIBottomPanel::new(),
            left_panel: UILeftPanel::new(),
            central_panel: UICentralPanel::new(),
        }
    }
}

impl eframe::App for ToniqueApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Update state
        self.state.update();
        if self.state.playback_state() == PlaybackState::Playing
            || self.state.preview_playback_state() == PlaybackState::Playing
        {
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.top_bar.show(ui, &mut self.state);
        self.bottom_panel.show(ui, &mut self.state);
        self.left_panel.show(ui, &mut self.state);
        self.central_panel.show(ui, &mut self.state);
    }
}
