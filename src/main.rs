use crate::{
    audio::host::{engine_without_audio, start_audio},
    config::settings::Settings,
    ui::run,
};

mod analysis;
mod audio;
mod cache;
mod config;
mod core;
mod ui;

pub mod utils;
mod waveform;
fn main() {
    // Audio output: the engine on the chosen device, or the default one if
    // that fails (e.g. the device was unplugged)
    let mut settings = Settings::load();
    let started = start_audio(&settings).or_else(|e| {
        eprintln!("Audio settings failed ({e}), using the default device");
        settings = Settings {
            ui_scale: settings.ui_scale,
            metronome_level: settings.metronome_level,
            ..Settings::default()
        };
        start_audio(&settings)
    });
    // Without any device the app still opens, and keeps trying
    let (audio, engine) = match started {
        Ok((audio, engine)) => (Ok(audio), engine),
        Err(e) => {
            eprintln!("No audio output: {e}");
            (Err(e.to_string()), engine_without_audio(&settings))
        }
    };
    // Ui thread (main thread). Opens the app window
    if let Err(e) = run(engine, audio, settings) {
        eprintln!("Couldn't open the window: {e}");
        std::process::exit(1);
    }
}
