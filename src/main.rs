use crate::{
    audio::{host::start_audio, midi::spawn_midi_thread},
    config::settings::Settings,
    ui::run,
};

use rtrb::RingBuffer;

mod analysis;
mod audio;
mod cache;
mod config;
mod core;
mod ui;

pub mod utils;
mod waveform;
fn main() {
    // Midi thread that collects midi inputs (not routed to the engine yet)
    let (midi_tx, _midi_rx) = RingBuffer::<Vec<u8>>::new(256);
    spawn_midi_thread(midi_tx);
    // Audio output: the engine on the chosen device, or the default one if
    // that fails (e.g. the device was unplugged)
    let mut settings = Settings::load();
    let (audio, engine) = start_audio(&settings).unwrap_or_else(|e| {
        eprintln!("Audio settings failed ({e}), using the default device");
        settings = Settings {
            ui_scale: settings.ui_scale,
            metronome_level: settings.metronome_level,
            ..Settings::default()
        };
        start_audio(&settings).expect("failed to start audio output")
    });
    // Ui thread (main thread). Opens the app window
    run(engine, audio, settings).unwrap();
}
