use crate::{
    audio::{host::start_audio, midi::spawn_midi_thread},
    ui::spawn_ui_thread,
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
    // Audio output: the engine on the default device
    let (_stream, engine) = start_audio().expect("failed to start audio output");
    // Ui thread (main thread). Opens the app window
    spawn_ui_thread(engine).unwrap();
}
