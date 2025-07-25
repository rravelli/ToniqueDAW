use std::fs::File;

use std::time::Instant;
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

const CHUNK_SIZE: usize = 88200;

fn normalize_buffer(audio_buf: AudioBufferRef, sample_buffer: &mut Vec<f32>, channel: usize) {
    match audio_buf {
        AudioBufferRef::U8(buf) => {
            sample_buffer.extend(
                buf.chan(channel)
                    .iter()
                    .map(|&sample| (sample as f32 - 128.0) / 128.0), // [-1.0, 1.0)
            );
        }
        AudioBufferRef::U16(buf) => {
            sample_buffer.extend(
                buf.chan(channel)
                    .iter()
                    .map(|&sample| (sample as f32 - 32768.0) / 32768.0),
            );
        }
        AudioBufferRef::U24(buf) => {
            sample_buffer.extend(
                buf.chan(channel)
                    .iter()
                    .map(|&sample| (sample.inner() as f32 - 8_388_608.0) / 8_388_608.0),
            );
        }
        AudioBufferRef::U32(buf) => {
            sample_buffer.extend(
                buf.chan(channel)
                    .iter()
                    .map(|&sample| (sample as f32 - 2_147_483_648.0) / 2_147_483_648.0),
            );
        }
        AudioBufferRef::S8(buf) => {
            sample_buffer.extend(
                buf.chan(channel)
                    .iter()
                    .map(|&sample| sample as f32 / -(i8::MIN as f32)), // i8::MIN = -128
            );
        }
        AudioBufferRef::S16(buf) => {
            sample_buffer.extend(
                buf.chan(channel)
                    .iter()
                    .map(|&sample| sample as f32 / -(i16::MIN as f32)), // i16::MIN = -32768
            );
        }
        AudioBufferRef::S24(buf) => {
            sample_buffer.extend(
                buf.chan(channel)
                    .iter()
                    .map(|&sample| sample.inner() as f32 / -(1 << 23) as f32), // = -8_388_608
            );
        }
        AudioBufferRef::S32(buf) => {
            sample_buffer.extend(
                buf.chan(channel)
                    .iter()
                    .map(|&sample| sample as f32 / -(i32::MIN as f32)), // i32::MIN = -2_147_483_648
            );
        }
        AudioBufferRef::F32(buf) => {
            sample_buffer.extend(buf.chan(channel)); // Already normalized
        }
        AudioBufferRef::F64(buf) => {
            sample_buffer.extend(
                buf.chan(channel).iter().map(|&sample| sample as f32), // Just downcast
            );
        }
    }
}

use std::sync::{Arc, Mutex};

pub fn load_audio(
    path: String,
    shared_data: Arc<Mutex<(Vec<f32>, Vec<f32>)>>,
    ready: Arc<Mutex<bool>>,
) -> Result<(), String> {
    let start = Instant::now();
    // Open the audio file
    let file = File::open(&path).map_err(|e| format!("Failed to open file: {}", e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    // Probe the file to detect format
    let hint = Hint::new();
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("Failed to probe file: {}", e))?;
    let mut format = probed.format;

    // Extract the track ID before the loop to avoid borrowing conflicts
    let track_id = {
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or("No valid audio tracks found")?;
        track.id
    };

    // Create a decoder for the track
    let mut decoder = symphonia::default::get_codecs()
        .make(&format.tracks()[0].codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Failed to create decoder: {}", e))?;

    let mut buffer_0 = Vec::new();
    let mut buffer_1 = Vec::new();

    // Decode the audio packets
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }

        // Decode the packet
        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                let audio_buf_copy = audio_buf.clone();
                // Process the decoded audio buffer on channel 0
                normalize_buffer(audio_buf, &mut buffer_0, 0);
                // Process the decoded audio buffer on channel 1
                if audio_buf_copy.spec().channels.count() > 1 {
                    normalize_buffer(audio_buf_copy, &mut buffer_1, 1);
                }
                // Append data chunk by chunk
                if buffer_0.len() > CHUNK_SIZE || buffer_1.len() > CHUNK_SIZE {
                    let mut data = shared_data.lock().unwrap();
                    data.0.extend(buffer_0.clone());
                    data.1.extend(buffer_1.clone());
                    buffer_0 = Vec::new();
                    buffer_1 = Vec::new();
                }
            }
            Err(e) => eprintln!("Error decoding audio packet: {}", e),
        }
    }
    // Add missing data
    let mut data = shared_data.lock().unwrap();
    data.0.extend(buffer_0.clone());
    data.1.extend(buffer_1.clone());
    // Now ready
    let mut ready = ready.lock().unwrap();
    *ready = true;
    let duration = start.elapsed();
    println!("Finished {} in: {:?}", path, duration);

    Ok(())
}
