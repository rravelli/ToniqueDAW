use std::collections::VecDeque;
use std::thread::sleep;
use std::time::Duration;

use crate::audio::player::PlayerBackend;
use crate::core::message::{AudioToGuiTx, GuiToAudioRx};
use crate::core::metrics::AudioMetrics;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Device, Stream, StreamConfig};
use crossbeam::channel::{Receiver, Sender};
use rtrb::{Consumer, Producer};

pub fn spawn_output_stream(
    to_gui_tx: AudioToGuiTx,
    from_gui_rx: GuiToAudioRx,
    midi_rx: Consumer<Vec<u8>>,
) -> cpal::Stream {
    // Setup cpal audio output

    let host = cpal::default_host();

    let device = host
        .default_output_device()
        .expect("no output device available");

    let sample_rate = device.default_output_config().unwrap().sample_rate();

    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate,
        buffer_size: BufferSize::Default,
    };

    let mut player = PlayerBackend::new(to_gui_tx, from_gui_rx, midi_rx, sample_rate.0 as usize);
    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| player.mix_audio(data),
            move |err| {
                eprintln!("{}", err);
            },
            None,
        )
        .unwrap();

    stream.play().unwrap();

    stream
}

pub fn spawn_output_thread(
    device: Device,
    mut rx: Consumer<OutputStreamMessage>,
    mut tx: Producer<usize>,
) -> Result<Stream, String> {
    let sample_rate = device.default_output_config().unwrap().sample_rate();
    let config = StreamConfig {
        channels: 2,
        sample_rate,
        buffer_size: BufferSize::Default,
    };

    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                data.fill(0.);
                let len = data.len();
                let _ = tx.push(len);
                sleep(Duration::from_millis(4));
                while let Ok(msg) = rx.pop() {
                    match msg {
                        OutputStreamMessage::AddChunk(items) => {
                            if !items.is_empty() {
                                let size = items.len().min(len);
                                data[..size].copy_from_slice(&items[..size]);
                            }
                        }
                    }
                }
            },
            |err| {},
            None,
        )
        .map_err(|err| err.to_string())?;

    stream.play().map_err(|err| err.to_string())?;

    Ok(stream)
}

pub enum OutputStreamMessage {
    AddChunk(Vec<f32>),
}

pub struct OutputStream {
    pub name: String,
    pub _stream: cpal::Stream,
    pub tx: Producer<OutputStreamMessage>,
    pub rx: Consumer<usize>,
    pub playhead: usize,
}
