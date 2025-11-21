use crate::{audio::spawn_audio_manager, core::message::GuiToPlayerMsg, ui::spawn_ui_thread};

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
    // Audio thread
    spawn_audio_manager(to_gui_sender, from_gui_rx);
    // Ui thread (main thread). Opens the app window
    spawn_ui_thread(to_process_tx, from_process_receiver).unwrap();
}
