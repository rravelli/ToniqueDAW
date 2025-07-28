use eframe::egui::{self, Layout, Vec2};
use egui::{Frame, Margin, Pos2};
use rtrb::{Consumer, Producer};

use crate::{
    components::workspace::Workspace,
    message::{GuiToPlayerMsg, ProcessToGuiMsg},
};

pub struct ToniqueApp {
    workspace: Workspace,
}

impl ToniqueApp {
    pub fn new(
        to_player_tx: Producer<GuiToPlayerMsg>,
        from_player_rx: Consumer<ProcessToGuiMsg>,
        _cc: &eframe::CreationContext<'_>,
    ) -> Self {
        let workspace = Workspace::new(to_player_tx, from_player_rx);
        Self { workspace }
    }
}
impl eframe::App for ToniqueApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.style_mut(|style| {
            style.spacing.item_spacing = Vec2::ZERO;
            style.spacing.window_margin = Margin::ZERO
        });

        egui::CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).inner_margin(Margin::ZERO))
            .show(ctx, |ui| {
                egui::warn_if_debug_build(ui);
                ui.label(format!(
                    "FPS: {:.1}",
                    1.0 / ui.ctx().input(|i| i.stable_dt).max(1e-5)
                ));
                self.workspace.ui(ui);
            });
    }
}
