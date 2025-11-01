use crate::{
    audio::midi::spawn_midi_thread,
    core::message::{GuiToPlayerMsg, ProcessToGuiMsg},
    ui::spawn_ui_thread,
};

use rtrb::RingBuffer;

mod analysis;
mod audio;
mod cache;
mod config;
mod core;
mod output;
mod ui;

pub mod utils;
mod waveform;
fn main() {
    // Create channels
    let (to_gui_tx, from_process_rx) = RingBuffer::<ProcessToGuiMsg>::new(256);
    let (to_process_tx, from_gui_rx) = RingBuffer::<GuiToPlayerMsg>::new(256);
    let (midi_tx, midi_rx) = RingBuffer::<Vec<u8>>::new(256);
    // Midi thread that collects midi inputs
    spawn_midi_thread(midi_tx);
    // Audio thread that plays sound to the device
    let _cpal_stream = output::spawn_cpal_stream(to_gui_tx, from_gui_rx, midi_rx);
    // Ui thread (main thread). Opens the app window
    spawn_ui_thread(to_process_tx, from_process_rx).unwrap();
}
