use crate::audio::player::PlayerBackend;
use crate::core::message::{GuiToPlayerMsg, ProcessToGuiMsg};
use cpal::BufferSize;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, Producer};

pub fn spawn_cpal_stream(
    to_gui_tx: Producer<ProcessToGuiMsg>,
    from_gui_rx: Consumer<GuiToPlayerMsg>,
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
