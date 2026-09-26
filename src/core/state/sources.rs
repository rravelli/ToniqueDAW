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

        let (audio, rate, tx) = (audio.clone(), self.engine_rate, self.loaded_tx.clone());
        std::thread::spawn(move || {
            if let Some(buffer) = convert_when_ready(&audio, rate) {
                let _ = tx.send((id, Arc::new(buffer)));
            }
        });
        id
    }

    pub fn info(&self, id: SourceId) -> Option<&AudioInfo> {
        self.info.get(&id)
    }

    /// Sources whose data became available since the last call.
    pub fn poll_loaded(&self) -> Vec<(SourceId, Arc<SampleBuffer>)> {
        self.loaded_rx.try_iter().collect()
    }
}

fn convert_when_ready(audio: &AudioInfo, engine_rate: f64) -> Option<SampleBuffer> {
    let started = Instant::now();
    while !audio.ready.read().is_ok_and(|r| *r) {
        if started.elapsed() > LOAD_TIMEOUT {
            eprintln!("Gave up waiting for {} to decode", audio.name);
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let data = audio.data.read().ok()?;
    let channels = if data.1.is_empty() {
        vec![data.0.clone()]
    } else {
        vec![data.0.clone(), data.1.clone()]
    };
    let buffer = SampleBuffer::new(channels, audio.sample_rate as f64);
    Some(buffer.resampled(engine_rate))
}
