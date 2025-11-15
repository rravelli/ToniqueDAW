use crate::{
    audio::{midi::spawn_midi_thread, spawn_audio_manager, spawn_audio_thread},
    core::message::{GuiToPlayerMsg, ProcessToGuiMsg},
    ui::spawn_ui_thread,
};

use crossbeam::channel::unbounded;
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
    // Create channels
    let (to_gui_sender, from_process_receiver) = unbounded();
    let (to_process_tx, from_gui_rx) = RingBuffer::<GuiToPlayerMsg>::new(256);
    let (midi_tx, midi_rx) = RingBuffer::<Vec<u8>>::new(256);

    // Midi thread that collects midi inputs
    spawn_midi_thread(midi_tx);
    // Audio thread
    // let (input, ouput) = spawn_audio_thread(to_gui_sender, from_gui_rx, midi_rx);

    spawn_audio_manager(to_gui_sender, from_gui_rx, midi_rx);
    // Ui thread (main thread). Opens the app window
    spawn_ui_thread(to_process_tx, from_process_receiver).unwrap();
}
