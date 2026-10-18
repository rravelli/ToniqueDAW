use directories::ProjectDirs;
use std::collections::hash_map::DefaultHasher;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Weak;
use std::time::Instant;
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tonique_engine::peaks::{PeakBuilder, WaveformPeaks};
use tonique_engine::sample::SampleBuffer;

use crate::analysis::AudioData;

/// Minimum number of frames decoded between two published peak snapshots.
const CHUNK_SIZE: usize = 88200;

fn normalize_buffer(audio_buf: &AudioBufferRef, sample_buffer: &mut Vec<f32>, channel: usize) {
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

/// Decode `path` into `data`: peaks are published progressively (unless
/// they're already complete), samples once the whole file is decoded if
/// `keep_samples`. Stops early once `data` is dropped, or for a peaks-only
/// decode, once a decode keeping the samples takes over.
pub fn load_audio(path: &Path, data: &Weak<AudioData>, keep_samples: bool) -> Result<(), String> {
    let start = Instant::now();
    // Open the audio file
    let file = File::open(path).map_err(|e| format!("Failed to open file: {}", e))?;
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

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or("No valid audio tracks found")?;
    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or("Missing sample rate")? as f64;
    // Only an estimate for some formats; the buffers are trimmed at the end.
    let expected_frames = track.codec_params.n_frames.unwrap_or(0) as usize;

    // Create a decoder for the track
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Failed to create decoder: {}", e))?;

    let Some(peaks_complete) = data.upgrade().map(|d| d.peaks_complete()) else {
        return Ok(());
    };
    let mut channels: Vec<Vec<f32>> = Vec::new();
    let mut scratch: Vec<Vec<f32>> = Vec::new();
    let mut builder: Option<PeakBuilder> = None;
    let mut next_publish = CHUNK_SIZE;

    // Decode the audio packets
    while let Ok(packet) = format.next_packet() {
        // Only hold `data` for an instant: while held, the file looks in use
        // and the cache won't let go of it.
        let superseded = match data.upgrade() {
            None => true,
            Some(data) => !keep_samples && data.samples_requested(),
        };
        if superseded {
            return Ok(());
        }
        if packet.track_id() != track_id {
            continue;
        }

        let audio_buf = match decoder.decode(&packet) {
            Ok(audio_buf) => audio_buf,
            Err(e) => {
                eprintln!("Error decoding audio packet: {}", e);
                continue;
            }
        };
        let num_channels = audio_buf.spec().channels.count();
        if scratch.is_empty() {
            if keep_samples {
                channels = (0..num_channels)
                    .map(|_| Vec::with_capacity(expected_frames))
                    .collect();
            }
            scratch = vec![Vec::new(); num_channels];
            builder = (!peaks_complete).then(|| PeakBuilder::new(num_channels, sample_rate));
        }
        if num_channels != scratch.len() {
            eprintln!("Skipping packet with a different channel count");
            continue;
        }
        for (ch, buf) in scratch.iter_mut().enumerate() {
            buf.clear();
            normalize_buffer(&audio_buf, buf, ch);
        }
        for (all, block) in channels.iter_mut().zip(&scratch) {
            all.extend_from_slice(block);
        }
        if let Some(builder) = &mut builder {
            let block: Vec<&[f32]> = scratch.iter().map(Vec::as_slice).collect();
            builder.push(&block);
            // Geometric spacing keeps the total snapshot copying linear.
            if builder.frames() >= next_publish {
                if let Some(data) = data.upgrade() {
                    data.publish_peaks(builder.snapshot());
                }
                next_publish = builder.frames() + CHUNK_SIZE.max(builder.frames() / 2);
            }
        }
    }
    if scratch.is_empty() {
        return Err("No audio decoded".into());
    }
    let Some(data) = data.upgrade() else {
        return Ok(());
    };
    if let Some(builder) = builder {
        let peaks = builder.snapshot();
        store_cached_peaks(path, &peaks);
        data.complete_peaks(peaks);
    }
    if keep_samples {
        // Drop the slack left by an estimated length or by growing.
        channels.iter_mut().for_each(Vec::shrink_to_fit);
        data.finish(SampleBuffer::new(channels, sample_rate));
    }
    println!("Finished {} in: {:?}", path.display(), start.elapsed());

    Ok(())
}

/// Peaks of `path` saved by a previous run, if the file hasn't changed.
pub fn load_cached_peaks(path: &Path) -> Option<WaveformPeaks> {
    let bytes = std::fs::read(peak_cache_path(path)?).ok()?;
    WaveformPeaks::from_bytes(&bytes)
}

fn store_cached_peaks(path: &Path, peaks: &WaveformPeaks) {
    let Some(cache_path) = peak_cache_path(path) else {
        return;
    };
    let result = cache_path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|_| std::fs::write(&cache_path, peaks.to_bytes()));
    if let Err(e) = result {
        eprintln!("Failed to cache peaks of {}: {e}", path.display());
    }
}

/// Cache file keyed by the file's path, size and modification time.
fn peak_cache_path(path: &Path) -> Option<PathBuf> {
    let meta = std::fs::metadata(path).ok()?;
    let mut hasher = DefaultHasher::new();
    path.canonicalize().ok()?.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    meta.modified().ok()?.hash(&mut hasher);
    let dirs = ProjectDirs::from("com", "Bytenosis", "Tonique")?;
    Some(
        dirs.cache_dir()
            .join("peaks")
            .join(format!("{:016x}.peaks", hasher.finish())),
    )
}
