use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{BufferSize, Device, Stream, StreamConfig};
use rtrb::{Consumer, Producer};
use std::thread::sleep;
use std::time::Duration;

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
            |err| eprintln!("Output error: {}", err),
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
