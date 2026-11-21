use crate::ui::{app::ToniqueApp, font::fonts, window::native_options};
use crate::{audio::host::AudioHost, config::settings::Settings};
use tonique_engine::engine::Engine;
mod about;
mod app;
mod arrangement;
mod browser;
mod clip_details;
mod commands;
mod dnd;
mod effects;
mod export;
mod font;
mod graph;
mod keymap;
mod missing_files;
mod panels;
mod project;
mod settings;
mod taskbar;
mod theme;
mod waveform;
mod widget;
mod window;
mod workspace;

/// The grid maps beats onto egui's rectangles: what it needs of a view is
/// its left and right edges.
impl crate::core::grid::Viewport for egui::Rect {
    fn left(&self) -> f32 {
        self.min.x
    }
    fn right(&self) -> f32 {
        self.max.x
    }
}

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
            egui_extras::install_image_loaders(&cc.egui_ctx);
            cc.egui_ctx.set_zoom_factor(settings.ui_scale);
            Ok(Box::new(ToniqueApp::new(engine, audio, settings, cc)))
        }),
    )
}
