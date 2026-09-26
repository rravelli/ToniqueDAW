use crate::audio::preview::PreviewBackend;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, Producer, RingBuffer};
use std::path::PathBuf;
use tonique_engine::{
    engine::{AudioProcessor, Engine, EngineConfig},
    rt,
};

/// Commands for the file-browser preview, which plays outside the engine.
#[derive(Debug)]
pub enum PreviewCommand {
    Play(PathBuf),
    Pause,
    Seek(usize),
}

/// UI side of the preview player.
pub struct PreviewLink {
    pub tx: Producer<PreviewCommand>,
    /// Preview playhead, in frames of the previewed file.
    pub position_rx: Consumer<usize>,
}

/// Keeps audio running; drop it to stop the stream.
pub struct AudioHost {
    _stream: cpal::Stream,
}

/// Open the default output device, start the engine on it and return the
/// control-side handles.
pub fn start_audio() -> Result<(AudioHost, Engine, PreviewLink), Box<dyn std::error::Error>> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no output device available")?;
    let config = device.default_output_config()?.config();
    let sample_rate = config.sample_rate;
    let channels = config.channels as usize;

    let (engine, processor) = Engine::new(EngineConfig {
        sample_rate: sample_rate as f64,
        max_block: 256,
        worker_threads: 2,
        ..Default::default()
    });
    let (preview_tx, preview_rx) = RingBuffer::new(64);
    let (position_tx, position_rx) = RingBuffer::new(64);

    let callback = audio_callback(
        processor,
        preview_rx,
        position_tx,
        channels,
        sample_rate as usize,
    );
    let stream = device.build_output_stream::<f32, _, _>(
        config,
        callback,
        |err| eprintln!("audio stream error: {err}"),
        None,
    )?;
    stream.play()?;

    let preview = PreviewLink {
        tx: preview_tx,
        position_rx,
    };
    Ok((AudioHost { _stream: stream }, engine, preview))
}

/// Render the engine, then mix the preview on top. Only the engine part is
/// real-time safe; preview decoding runs here as it did before the engine.
fn audio_callback(
    mut processor: AudioProcessor,
    mut preview_rx: Consumer<PreviewCommand>,
    mut position_tx: Producer<usize>,
    channels: usize,
    sample_rate: usize,
) -> impl FnMut(&mut [f32], &cpal::OutputCallbackInfo) + Send + 'static {
    let mut first = true;
    let mut preview = PreviewBackend::new();
    let mut preview_playing = false;
    move |data, _| {
        if first {
            first = false;
            rt::enable_flush_denormals();
            // Best effort: needs rtkit or an rtprio limit.
            let _ = rt::set_realtime_priority(70);
        }
        processor.process_interleaved(data, channels);

        while let Ok(cmd) = preview_rx.pop() {
            match cmd {
                PreviewCommand::Play(file) => {
                    preview.play(file);
                    preview_playing = true;
                }
                PreviewCommand::Pause => preview_playing = false,
                PreviewCommand::Seek(pos) => {
                    preview.seek(pos);
                    preview_playing = true;
                }
            }
        }
        if preview_playing && let Some(samples) = preview.read(data.len() / channels, sample_rate) {
            for (frame, (l, r)) in data
                .chunks_exact_mut(channels)
                .zip(samples[0].iter().zip(samples[1].iter()))
            {
                frame[0] += l;
                if channels > 1 {
                    frame[1] += r;
                }
            }
            if let Some(stream) = &preview.stream {
                let _ = position_tx.push(stream.playhead());
            }
        }
    }
}
