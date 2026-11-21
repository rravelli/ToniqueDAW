use creek::{ReadDiskStream, ReadStreamOptions, SeekMode, SymphoniaDecoder, read::ReadError};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tonique_engine::preview::PreviewSource;

/// Longest wait for the disk stream to buffer before giving up (silence).
const BUFFER_TIMEOUT: Duration = Duration::from_secs(2);

/// Streams an audio file from disk for the engine's preview player. Runs on
/// the engine's preview thread, so waiting for the disk is fine here.
pub struct FilePreview {
    stream: Box<ReadDiskStream<SymphoniaDecoder>>,
    sample_rate: f64,
}

impl FilePreview {
    pub fn open(path: PathBuf) -> Option<Self> {
        let mut stream = Box::new(ReadDiskStream::new(path, 0, ReadStreamOptions::default()).ok()?);
        let _ = stream.cache(0, 0);
        let sample_rate = stream.info().sample_rate? as f64;
        Some(Self {
            stream,
            sample_rate,
        })
    }

    fn wait_until_ready(&mut self) -> bool {
        let started = Instant::now();
        while !self.stream.is_ready().unwrap_or(false) {
            if started.elapsed() > BUFFER_TIMEOUT {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        true
    }
}

impl PreviewSource for FilePreview {
    fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    fn seek(&mut self, frame: usize) {
        let _ = self.stream.seek(frame, SeekMode::Auto);
    }

    fn read(&mut self, left: &mut [f32], right: &mut [f32]) -> usize {
        loop {
            if !self.wait_until_ready() {
                return 0;
            }
            match self.stream.read(left.len()) {
                Ok(data) => {
                    let n = data.num_frames();
                    left[..n].copy_from_slice(data.read_channel(0));
                    let r = if data.num_channels() > 1 { 1 } else { 0 };
                    right[..n].copy_from_slice(data.read_channel(r));
                    return n;
                }
                // The disk thread is busy: try again shortly.
                Err(ReadError::IOServerChannelFull) => std::thread::sleep(Duration::from_millis(1)),
                Err(_) => return 0,
            }
        }
    }
}
