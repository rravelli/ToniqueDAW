use eframe::egui::Vec2;
use egui::Margin;
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

        self.workspace.show(ctx);
    }
}
