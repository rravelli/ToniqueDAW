use eframe::egui::Vec2;
use egui::{Color32, CornerRadius, Margin, Shadow, Stroke};
use rtrb::{Consumer, Producer};

use crate::{
    message::{GuiToPlayerMsg, ProcessToGuiMsg},
    ui::workspace::Workspace,
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
            style.spacing.window_margin = Margin::ZERO;
            style.spacing.menu_margin = Margin::same(4);
            style.visuals.menu_corner_radius = CornerRadius::same(2);
            style.visuals.popup_shadow = Shadow::NONE;
            style.visuals.window_stroke = Stroke::new(0.5, Color32::from_white_alpha(200));
        });
        self.workspace.show(ctx);
    }
}
