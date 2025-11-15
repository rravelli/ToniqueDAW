use egui::{Color32, Context, Frame, Ui, include_image};

use crate::core::state::ToniqueProjectState;

pub struct UIDecorationPanel {}

impl UIDecorationPanel {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show(&mut self, ctx: &Context, state: &mut ToniqueProjectState) {
        egui::TopBottomPanel::top("top-bar")
            .resizable(false)
            .frame(Frame::new().fill(Color32::from_gray(40)))
            .show(ctx, |ui| {
                self.ui(ui);
            });
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        ui.image(include_image!("../../../images/logo.png"));
    }
}
