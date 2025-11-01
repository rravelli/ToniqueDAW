use crate::{
    core::{
        message::{AudioToGuiRx, GuiToAudioTx},
        state::{PlaybackState, ToniqueProjectState},
    },
    ui::panels::{
        bottom_panel::UIBottomPanel, central_panel::UICentralPanel, left_panel::UILeftPanel,
        top_bar::UITopBar,
    },
};

pub struct ToniqueApp {
    state: ToniqueProjectState,
    top_bar: UITopBar,
    bottom_panel: UIBottomPanel,
    left_panel: UILeftPanel,
    central_panel: UICentralPanel,
}

impl ToniqueApp {
    pub fn new(tx: GuiToAudioTx, rx: AudioToGuiRx, _cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            state: ToniqueProjectState::new(tx, rx),
            top_bar: UITopBar::new(),
            bottom_panel: UIBottomPanel::new(),
            left_panel: UILeftPanel::new(),
            central_panel: UICentralPanel::new(),
        }
    }
}

impl eframe::App for ToniqueApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Update state
        self.state.update();
        if self.state.playback_state() == PlaybackState::Playing
            || self.state.preview_playback_state() == PlaybackState::Playing
        {
            ctx.request_repaint();
        }
        self.top_bar.show(ctx, &mut self.state);
        self.bottom_panel.show(ctx, &mut self.state);
        self.left_panel.show(ctx, &mut self.state);
        self.central_panel.show(ctx, &mut self.state);
    }
}
