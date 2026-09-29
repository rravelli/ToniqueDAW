use crate::ui::{app::ToniqueApp, font::fonts, window::native_options};
use crate::{audio::host::AudioHost, config::settings::Settings};
use tonique_engine::engine::Engine;
mod app;
mod arrangement;
mod browser;
mod commands;
mod dnd;
mod effects;
mod font;
mod graph;
mod panels;
mod project;
mod settings;
mod theme;
mod waveform;
mod widget;
mod window;
mod workspace;

/// Recording isn't implemented yet: hides the record and arm buttons.
const RECORDING: bool = false;

pub fn run(
    engine: Engine,
    audio: Result<AudioHost, String>,
    settings: Settings,
) -> Result<(), eframe::Error> {
    eframe::run_native(
        "Tonique",
        native_options(),
        Box::new(|cc| {
            cc.egui_ctx.set_fonts(fonts());
            cc.egui_ctx.set_zoom_factor(settings.ui_scale);
            Ok(Box::new(ToniqueApp::new(engine, audio, settings, cc)))
        }),
    )
}
