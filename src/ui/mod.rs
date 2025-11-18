use crate::{
    core::message::{AudioToGuiRx, GuiToAudioTx},
    ui::{app::ToniqueApp, font::get_fonts, theme::get_app_style, window::get_native_options},
};

use eframe::egui::ThemePreference;
pub mod app;
mod buttons;
mod clip;
pub mod effect;
pub mod effects;
pub mod font;
pub mod panels;
mod theme;
mod track;
mod utils;
mod view;
mod waveform;
mod widget;
mod window;

pub fn spawn_ui_thread(tx: GuiToAudioTx, rx: AudioToGuiRx) -> Result<(), eframe::Error> {
    eframe::run_native(
        "Tonique",
        get_native_options(),
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            cc.egui_ctx.set_fonts(get_fonts());
            cc.egui_ctx.set_style(get_app_style());
            cc.egui_ctx.set_theme(ThemePreference::Dark);
            Ok(Box::new(ToniqueApp::new(tx, rx, cc)))
        }),
    )
}
