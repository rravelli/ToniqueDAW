use egui::ViewportCommand;

use crate::{
    core::{
        message::{AudioToGuiRx, GuiToAudioTx},
        state::{PlaybackState, ToniqueProjectState},
    },
    ui::{
        panels::{
            bottom_panel::UIBottomPanel, central_panel::UICentralPanel,
            decoration_panel::UIDecorationPanel, left_panel::UILeftPanel, top_bar::UITopBar,
        },
        view::export::ExporWindow,
    },
};

pub struct ToniqueApp {
    state: ToniqueProjectState,
    decoration: UIDecorationPanel,
    top_bar: UITopBar,
    bottom_panel: UIBottomPanel,
    left_panel: UILeftPanel,
    central_panel: UICentralPanel,
    export_window: ExporWindow,
}

impl ToniqueApp {
    pub fn new(tx: GuiToAudioTx, rx: AudioToGuiRx, _cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            state: ToniqueProjectState::new(tx, rx),
            decoration: UIDecorationPanel::new(),
            top_bar: UITopBar::new(),
            bottom_panel: UIBottomPanel::new(),
            left_panel: UILeftPanel::new(),
            central_panel: UICentralPanel::new(),
            export_window: ExporWindow::new(),
        }
    }
}

impl eframe::App for ToniqueApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Update state
        self.state.update(ctx.input(|i| i.stable_dt));
        ctx.request_repaint();
        self.decoration.show(ctx, &mut self.state);
        self.top_bar.show(ctx, &mut self.state);
        self.bottom_panel.show(ctx, &mut self.state);
        self.left_panel.show(ctx, &mut self.state);
        self.central_panel.show(ctx, &mut self.state);

        if self.state.show_export {
            self.export_window.show(ctx, &mut self.state);
        }
    }
}
