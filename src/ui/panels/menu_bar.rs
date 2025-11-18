use crate::{
    core::state::ToniqueProjectState,
    ui::widget::context_menu::{ContextMenuButton, ContextMenuSeparator},
};
use egui::{
    Button, Color32, Context, Frame, Margin, RichText, Stroke, Ui, containers::menu::MenuButton,
    vec2,
};
use egui_phosphor::fill::{EXPORT, FLOPPY_DISK};

pub struct UIMenuBar {}

impl UIMenuBar {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show(&mut self, ctx: &Context, state: &mut ToniqueProjectState) {
        egui::TopBottomPanel::top("manu-bar")
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(Color32::from_gray(240))
                    .stroke(Stroke::NONE)
                    .inner_margin(Margin::symmetric(4, 0)),
            )
            .show(ctx, |ui| {
                self.ui(ui, state);
            });
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.spacing_mut().button_padding = vec2(2., 1.);
            self.file_menu(ui, state);
        });
    }

    fn file_menu(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        MenuButton::from_button(
            Button::new(
                RichText::new("File")
                    .size(10.)
                    .color(Color32::from_gray(40)),
            )
            .small()
            .fill(Color32::TRANSPARENT),
        )
        .ui(ui, |ui| {
            ContextMenuButton::new("", "Open Recent").submenu(ui, |ui| {
                ui.add(ContextMenuButton::new("", "My project lol"))
            });
            ui.add(ContextMenuButton::new(FLOPPY_DISK, "Save"));
            ui.add(ContextMenuSeparator::new());
            if ui.add(ContextMenuButton::new(EXPORT, "Export")).clicked() {
                state.show_export = true;
            }
        });
    }
}
