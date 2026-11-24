use directories::ProjectDirs;
use std::collections::hash_map::DefaultHasher;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Weak;
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tonique_engine::onset::{OnsetBuilder, OnsetCurve};
use tonique_engine::peaks::{PeakBuilder, WaveformPeaks};
use tonique_engine::sample::SampleBuffer;

use crate::audio_files::analysis::AudioData;

/// Extensions of the formats [`load_audio`] decodes: those of the symphonia
/// features built in (its defaults, and mp3 through creek). Update it with
/// them.
pub const DECODED_EXTENSIONS: &[&str] = &["wav", "wave", "flac", "ogg", "oga", "mp3"];

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
/// they're already complete), the rhythm once the whole file is decoded
/// (unless it's known), and the samples then too if `keep_samples`. Stops early once `data` is dropped, or for a peaks-only
/// decode, once a decode keeping the samples takes over.
pub fn load_audio(path: &Path, data: &Weak<AudioData>, keep_samples: bool) -> Result<(), String> {
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

    let Some((peaks_complete, rhythm_complete)) = data
        .upgrade()
        .map(|d| (d.peaks_complete(), d.rhythm_complete()))
    else {
        return Ok(());
    };
    let mut channels: Vec<Vec<f32>> = Vec::new();
    let mut scratch: Vec<Vec<f32>> = Vec::new();
    let mut builder: Option<PeakBuilder> = None;
    let mut onsets: Option<OnsetBuilder> = None;
    let mut next_publish = CHUNK_SIZE;
    // Packets that couldn't be decoded, or changed channel count: told
    // once, at the end, however many.
    let mut skipped = 0usize;

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
            Err(_) => {
                skipped += 1;
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
            onsets = (!rhythm_complete).then(|| OnsetBuilder::new(num_channels, sample_rate));
        }
        if num_channels != scratch.len() {
            skipped += 1;
            continue;
        }
        for (ch, buf) in scratch.iter_mut().enumerate() {
            buf.clear();
            normalize_buffer(&audio_buf, buf, ch);
        }
        for (all, block) in channels.iter_mut().zip(&scratch) {
            all.extend_from_slice(block);
        }
        let block: Vec<&[f32]> = scratch.iter().map(Vec::as_slice).collect();
        if let Some(onsets) = &mut onsets {
            onsets.push(&block);
        }
        if let Some(builder) = &mut builder {
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
    if skipped > 0 {
        eprintln!(
            "Skipped {skipped} packets of {} that couldn't be decoded",
            path.display()
        );
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
    if let Some(onsets) = onsets {
        let onsets = onsets.finish();
        store_cached(path, ONSET_CACHE, &onsets.to_bytes());
        data.complete_rhythm(onsets);
    }
    if keep_samples {
        // Drop the slack left by an estimated length or by growing.
        channels.iter_mut().for_each(Vec::shrink_to_fit);
        data.finish(SampleBuffer::new(channels, sample_rate));
    }
    Ok(())
}

/// What is cached of a file: in a folder of its own, under an extension.
struct CacheKind {
    folder: &'static str,
    extension: &'static str,
    what: &'static str,
}

const PEAK_CACHE: CacheKind = CacheKind {
    folder: "peaks",
    extension: "peaks",
    what: "peaks",
};

const ONSET_CACHE: CacheKind = CacheKind {
    folder: "onsets",
    extension: "onsets",
    what: "onsets",
};

/// Peaks of `path` saved by a previous run, if the file hasn't changed.
pub fn load_cached_peaks(path: &Path) -> Option<WaveformPeaks> {
    WaveformPeaks::from_bytes(&load_cached(path, PEAK_CACHE)?)
}

/// Onsets of `path` saved by a previous run, if the file hasn't changed.
pub fn load_cached_onsets(path: &Path) -> Option<OnsetCurve> {
    OnsetCurve::from_bytes(&load_cached(path, ONSET_CACHE)?)
}

fn store_cached_peaks(path: &Path, peaks: &WaveformPeaks) {
    store_cached(path, PEAK_CACHE, &peaks.to_bytes());
}

fn load_cached(path: &Path, kind: CacheKind) -> Option<Vec<u8>> {
    std::fs::read(cache_path(path, &kind)?).ok()
}

fn store_cached(path: &Path, kind: CacheKind, bytes: &[u8]) {
    let Some(cache_path) = cache_path(path, &kind) else {
        return;
    };
    let result = cache_path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|_| std::fs::write(&cache_path, bytes));
    if let Err(e) = result {
        eprintln!("Failed to cache {} of {}: {e}", kind.what, path.display());
    }
}

/// Cache file keyed by the file's path, size and modification time.
fn cache_path(path: &Path, kind: &CacheKind) -> Option<PathBuf> {
    let meta = std::fs::metadata(path).ok()?;
    let mut hasher = DefaultHasher::new();
    path.canonicalize().ok()?.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    meta.modified().ok()?.hash(&mut hasher);
    let dirs = ProjectDirs::from("com", "Bytenosis", "Tonique")?;
    Some(dirs.cache_dir().join(kind.folder).join(format!(
        "{:016x}.{}",
        hasher.finish(),
        kind.extension
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_files::analysis::read_audio_info;
    use std::time::{Duration, Instant};
    use tonique_engine::sample::WavWriter;

    /// A fresh stereo WAV of `bars` of 4/4 at `bpm`, a click on each beat
    /// (fresh so nothing is cached for it).
    fn write_clicks(name: &str, bpm: f64, bars: f64) -> PathBuf {
        const RATE: f64 = 44100.;
        let dir = std::env::temp_dir().join(format!("tonique-rhythm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{name}.wav"));
        let beat = 60. / bpm * RATE;
        let frames = (bars * 4. * beat).round() as usize;
        let mut wav = WavWriter::create(&path, 2, RATE).unwrap();
        let samples: Vec<f32> = (0..frames)
            .flat_map(|frame| {
                let since = frame as f64 % beat;
                let click = (since * 0.3).sin() * (-since / 300.).exp();
                [click as f32 * 0.8; 2]
            })
            .collect();
        wav.write_interleaved(&samples).unwrap();
        wav.finalize().unwrap();
        path
    }

    fn wait_for(what: &str, done: impl Fn() -> bool) {
        let start = Instant::now();
        while !done() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "timed out: {what}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Browsing a loop finds its tempo, and caches it with the peaks.
    #[test]
    fn browsing_finds_the_tempo() {
        let path = write_clicks("clicks", 128., 2.);
        let info = read_audio_info(&path).unwrap();
        assert_eq!(info.tempo(), None, "not analysed yet");
        info.load_peaks();
        wait_for("rhythm", || info.data.analysed());
        let tempo = info.tempo().expect("a tempo");
        assert!((tempo - 128.).abs() < 0.05, "found {tempo}");
        assert!(info.data.samples().is_none(), "browsing keeps no samples");

        let again = read_audio_info(&path).unwrap();
        assert!(again.data.analysed(), "from the cache");
        assert_eq!(again.tempo(), Some(tempo));
    }

    /// Peaks cached by a build that found no onsets: browsing decodes the
    /// file again, for its onsets only.
    #[test]
    fn cached_peaks_alone_are_completed() {
        let path = write_clicks("peaks-only", 100., 4.);
        let first = read_audio_info(&path).unwrap();
        first.load_peaks();
        wait_for("first analysis", || first.data.analysed());
        std::fs::remove_file(cache_path(&path, &ONSET_CACHE).unwrap()).unwrap();

        let info = read_audio_info(&path).unwrap();
        assert!(info.data.peaks_complete() && !info.data.rhythm_complete());
        info.load_peaks();
        wait_for("onsets", || info.data.rhythm_complete());
        let tempo = info.tempo().expect("a tempo");
        assert!((tempo - 100.).abs() < 0.05, "found {tempo}");
    }
}
