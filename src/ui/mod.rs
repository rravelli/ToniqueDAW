use crate::ui::{
    app::ToniqueApp, font::get_fonts, theme::get_app_style, window::get_native_options,
};
use egui::Theme;
use tonique_engine::engine::Engine;
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

pub fn spawn_ui_thread(engine: Engine) -> Result<(), eframe::Error> {
    eframe::run_native(
        "Tonique",
        get_native_options(),
        Box::new(|cc| {
            cc.egui_ctx.set_fonts(get_fonts());
            cc.egui_ctx.set_global_style(get_app_style());
            cc.egui_ctx.set_theme(Theme::Dark);
            Ok(Box::new(ToniqueApp::new(engine, cc)))
        }),
    )
}
