use crate::{dev::memory::SourceMemory, library::analysis::AudioInfo};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc, Weak,
        mpsc::{Receiver, Sender, channel},
    },
    time::{Duration, Instant},
};
use tonique_engine::{edit::SourceId, sample::SampleBuffer};

/// Give up on files whose decoding never finishes (e.g. decode errors).
const LOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// A conversion's result: `None` when the file never decoded.
type Converted = (SourceId, u64, Option<Arc<SampleBuffer>>);

/// Audio files used by clips. Each file gets one engine source; its data is
/// converted to the engine's sample rate on a background thread once the
/// file is decoded, and handed to the engine from [`Self::poll_loaded`].
pub struct SourceRegistry {
    by_path: HashMap<PathBuf, SourceId>,
    info: HashMap<SourceId, AudioInfo>,
    engine_rate: f64,
    loaded_tx: Sender<Converted>,
    loaded_rx: Receiver<Converted>,
    /// Each source's latest conversion: a result of an older one (another
    /// rate, the file before a relink) is dropped.
    conversions: HashMap<SourceId, u64>,
    /// Sources whose latest conversion is still to come.
    converting: HashSet<SourceId>,
    /// Sources the engine was given the latest conversion of: what plays.
    playable: HashSet<SourceId>,
    /// What the engine was given for each source, to measure it.
    given: HashMap<SourceId, Weak<SampleBuffer>>,
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
            conversions: HashMap::new(),
            converting: HashSet::new(),
            playable: HashSet::new(),
            given: HashMap::new(),
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

    /// Play `audio` for source `id` from now on: every clip using it does.
    /// For a missing file found again.
    pub fn relink(&mut self, id: SourceId, audio: AudioInfo) {
        if let Some(old) = self.info.get(&id)
            && self.by_path.get(&old.path) == Some(&id)
        {
            self.by_path.remove(&old.path);
        }
        self.by_path.entry(audio.path.clone()).or_insert(id);
        self.info.insert(id, audio.clone());
        self.convert(id, audio);
    }

    /// Sources whose file is missing, by file name.
    pub fn missing(&self) -> Vec<(SourceId, &AudioInfo)> {
        let mut missing: Vec<_> = self
            .info
            .iter()
            .filter(|(_, audio)| audio.missing)
            .map(|(id, audio)| (*id, audio))
            .collect();
        missing.sort_by(|a, b| (&a.1.name, &a.1.path).cmp(&(&b.1.name, &b.1.path)));
        missing
    }

    /// The engine now runs at `rate`: convert every file again.
    pub fn set_engine_rate(&mut self, rate: f64) {
        if rate == self.engine_rate {
            return;
        }
        self.engine_rate = rate;
        let sources: Vec<_> = self
            .info
            .iter()
            .map(|(id, audio)| (*id, audio.clone()))
            .collect();
        for (id, audio) in sources {
            self.convert(id, audio);
        }
    }

    /// Convert `audio` to the engine rate in the background, decoding it
    /// first if no one has yet.
    /// Until it's given to the engine, the source isn't playable: what the
    /// engine has is of another rate or file.
    fn convert(&mut self, id: SourceId, audio: AudioInfo) {
        let conversion = self.conversions.entry(id).or_default();
        *conversion += 1;
        let conversion = *conversion;
        self.playable.remove(&id);
        // Silent until located.
        if audio.missing {
            self.converting.remove(&id);
            return;
        }
        self.converting.insert(id);
        audio.load_samples();
        let (rate, tx) = (self.engine_rate, self.loaded_tx.clone());
        std::thread::spawn(move || {
            let buffer = convert_when_ready(&audio, rate);
            let _ = tx.send((id, conversion, buffer));
        });
    }

    /// Whether a file clips use is still decoding, or converting for the
    /// engine.
    pub fn loading(&self) -> bool {
        !self.converting.is_empty() || self.info.values().any(|audio| audio.data.is_loading())
    }

    /// Whether the engine has source `id`'s audio, at its rate: its clips
    /// play. A file decoded is not enough, it's converted after.
    pub fn playable(&self, id: SourceId) -> bool {
        self.playable.contains(&id)
    }

    pub fn info(&self, id: SourceId) -> Option<&AudioInfo> {
        self.info.get(&id)
    }

    /// Sources whose data became available since the last call, to give
    /// the engine: they're playable from then on.
    pub fn poll_loaded(&mut self) -> Vec<(SourceId, Arc<SampleBuffer>)> {
        let mut loaded = Vec::new();
        for (id, conversion, buffer) in self.loaded_rx.try_iter() {
            if self.conversions.get(&id) != Some(&conversion) {
                continue;
            }
            self.converting.remove(&id);
            let Some(buffer) = buffer else {
                continue;
            };
            self.given.insert(id, Arc::downgrade(&buffer));
            self.playable.insert(id);
            loaded.push((id, buffer));
        }
        loaded
    }

    /// What the files and the engine's copies of them take.
    pub fn memory(&self) -> SourceMemory {
        let mut memory = SourceMemory::default();
        for (id, audio) in &self.info {
            memory.files.add(&audio.data);
            let Some(given) = self.given.get(id).and_then(Weak::upgrade) else {
                continue;
            };
            let shared = audio
                .data
                .samples()
                .is_some_and(|samples| Arc::ptr_eq(samples, &given));
            if !shared {
                memory.engine_copies += 1;
                memory.engine_bytes += given.byte_size();
            }
        }
        memory
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
        if audio.data.failed() {
            return None;
        }
        if started.elapsed() > LOAD_TIMEOUT {
            eprintln!("Gave up waiting for {} to decode", audio.name);
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the engine was given, polling until `id` is playable, then a
    /// little longer for any older conversion to come.
    fn poll_until_playable(
        sources: &mut SourceRegistry,
        id: SourceId,
    ) -> Vec<(SourceId, Arc<SampleBuffer>)> {
        let started = Instant::now();
        let mut loaded = Vec::new();
        while !sources.playable(id) {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "conversion never came"
            );
            loaded.extend(sources.poll_loaded());
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(50));
        loaded.extend(sources.poll_loaded());
        loaded
    }

    #[test]
    fn decoded_is_not_playable_until_the_engine_has_it() {
        let mut sources = SourceRegistry::new(44100.);
        let audio = AudioInfo::silent(1.);
        let id = sources.get_or_insert(&audio, || SourceId(1));
        assert!(audio.data.is_ready());
        assert!(!sources.playable(id));
        assert!(sources.loading());

        let loaded = poll_until_playable(&mut sources, id);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].1.sample_rate(), 44100.);
        assert!(!sources.loading());
    }

    #[test]
    fn a_rate_change_makes_sources_unplayable_and_drops_older_conversions() {
        let mut sources = SourceRegistry::new(48000.);
        let id = sources.get_or_insert(&AudioInfo::silent(1.), || SourceId(1));
        poll_until_playable(&mut sources, id);

        sources.set_engine_rate(44100.);
        assert!(!sources.playable(id));
        sources.set_engine_rate(22050.);
        let loaded = poll_until_playable(&mut sources, id);
        assert!(
            loaded
                .iter()
                .all(|(_, buffer)| buffer.sample_rate() == 22050.)
        );
    }
}
