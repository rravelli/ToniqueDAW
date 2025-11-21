use crate::core::{
    message::{AudioToGuiTx, GuiToAudioRx},
    state::ToniqueProjectState,
};
use crossbeam::channel::unbounded;

mod export;
mod looping;
mod track;

fn setup_state() -> ToniqueProjectState {
    let (tx, _) = rtrb::RingBuffer::new(128);
    let (_, rx) = unbounded();
    ToniqueProjectState::new(tx, rx)
}

fn setup_state_with_channels() -> (ToniqueProjectState, GuiToAudioRx, AudioToGuiTx) {
    let (tx, audio_rx) = rtrb::RingBuffer::new(128);
    let (audio_tx, rx) = unbounded();
    (ToniqueProjectState::new(tx, rx), audio_rx, audio_tx)
}
