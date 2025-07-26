use eframe::egui::{self, Layout, Vec2};
use egui::{Frame, Margin, Pos2};
use rtrb::{Consumer, Producer};

use crate::{
    components::{filepicker::FilePicker, workspace::Workspace},
    message::{GuiToPlayerMsg, ProcessToGuiMsg},
};

pub struct ToniqueApp {
    workspace: Workspace,
    file_picker: FilePicker,
}

impl ToniqueApp {
    pub fn new(
        to_player_tx: Producer<GuiToPlayerMsg>,
        from_player_rx: Consumer<ProcessToGuiMsg>,
        _cc: &eframe::CreationContext<'_>,
    ) -> Self {
        let workspace = Workspace::new(to_player_tx, from_player_rx);
        Self {
            file_picker: FilePicker::new(),
            workspace,
        }
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
                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), ui.available_height()),
                    Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let (dragged_audio_info, is_released) = self.file_picker.ui(ui);
                        self.workspace.ui(ui, dragged_audio_info, is_released);
                    },
                );
            });
    }
}
