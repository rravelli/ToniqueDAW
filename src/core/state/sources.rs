use crate::analysis::AudioInfo;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{Receiver, Sender, channel},
    },
    time::{Duration, Instant},
};
use tonique_engine::{edit::SourceId, sample::SampleBuffer};

/// Give up on files whose decoding never finishes (e.g. decode errors).
const LOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// Audio files used by clips. Each file gets one engine source; its data is
/// converted to the engine's sample rate on a background thread once the
/// file is decoded, and handed to the engine from [`Self::poll_loaded`].
pub struct SourceRegistry {
    by_path: HashMap<PathBuf, SourceId>,
    info: HashMap<SourceId, AudioInfo>,
    engine_rate: f64,
    loaded_tx: Sender<(SourceId, Arc<SampleBuffer>)>,
    loaded_rx: Receiver<(SourceId, Arc<SampleBuffer>)>,
}

impl SourceRegistry {
    pub fn new(engine_rate: f64) -> Self {
        let (loaded_tx, loaded_rx) = channel();
        Self {
            by_path: HashMap::new(),
            info: HashMap::new(),
            engine_rate,
            loaded_tx,
            loaded_rx,
        }
    }

    /// The source for `audio`'s file, registering it (and starting the
    /// conversion) on first use. `new_id` reserves an ID in the edit.
    pub fn get_or_insert(
        &mut self,
        audio: &AudioInfo,
        new_id: impl FnOnce() -> SourceId,
    ) -> SourceId {
        if let Some(id) = self.by_path.get(&audio.path) {
            return *id;
        }
        let id = new_id();
        self.by_path.insert(audio.path.clone(), id);
        self.info.insert(id, audio.clone());
        self.convert(id, audio.clone());
        id
    }

    /// The engine now runs at `rate`: convert every file again.
    pub fn set_engine_rate(&mut self, rate: f64) {
        if rate == self.engine_rate {
            return;
        }
        self.engine_rate = rate;
        for (id, audio) in &self.info {
            self.convert(*id, audio.clone());
        }
    }

    /// Convert `audio` to the engine rate in the background.
    fn convert(&self, id: SourceId, audio: AudioInfo) {
        let (rate, tx) = (self.engine_rate, self.loaded_tx.clone());
        std::thread::spawn(move || {
            if let Some(buffer) = convert_when_ready(&audio, rate) {
                let _ = tx.send((id, buffer));
            }
        });
    }

    pub fn info(&self, id: SourceId) -> Option<&AudioInfo> {
        self.info.get(&id)
    }

    /// Sources whose data became available since the last call.
    pub fn poll_loaded(&self) -> Vec<(SourceId, Arc<SampleBuffer>)> {
        self.loaded_rx.try_iter().collect()
    }
}

fn convert_when_ready(audio: &AudioInfo, engine_rate: f64) -> Option<Arc<SampleBuffer>> {
    let started = Instant::now();
    loop {
        if let Some(samples) = audio.data.samples() {
            // Share the decoded data when no conversion is needed.
            return Some(if samples.sample_rate() == engine_rate {
                samples.clone()
            } else {
                Arc::new(samples.resampled(engine_rate))
            });
        }
        if started.elapsed() > LOAD_TIMEOUT {
            eprintln!("Gave up waiting for {} to decode", audio.name);
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
