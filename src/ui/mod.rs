use crate::ui::{app::ToniqueApp, font::get_fonts, window::get_native_options};
use crate::{audio::host::AudioHost, config::settings::Settings};
use tonique_engine::engine::Engine;
pub mod app;
mod buttons;
mod clip;
pub mod effect;
pub mod effects;
pub mod font;
pub mod panels;
pub mod theme;
mod track;
mod utils;
mod view;
mod waveform;
mod widget;
mod window;
pub mod windows;

pub fn spawn_ui_thread(
    engine: Engine,
    audio: AudioHost,
    settings: Settings,
) -> Result<(), eframe::Error> {
    eframe::run_native(
        "Tonique",
        get_native_options(),
        Box::new(|cc| {
            cc.egui_ctx.set_fonts(get_fonts());
            cc.egui_ctx.set_zoom_factor(settings.ui_scale);
            Ok(Box::new(ToniqueApp::new(engine, audio, settings, cc)))
        }),
    )
}
