//! Rendering the project to an audio file, on a background thread with its
//! own engine, so playback and editing go on meanwhile.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread::{self, JoinHandle},
};
use tonique_engine::{
    edit::{Edit, EditSession},
    engine::{Engine, EngineConfig},
    sample::{WavFormat, WavWriter},
    time::BeatPos,
};

/// Frames rendered and written at a time.
const CHUNK_FRAMES: usize = 8192;

/// What part of the project to export.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportRange {
    /// From the start to the end of the last clip.
    #[default]
    Project,
    /// The loop region.
    Loop,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExportOptions {
    pub range: ExportRange,
    pub format: WavFormat,
}

/// How an export ended.
#[derive(Clone, Debug, PartialEq)]
pub enum ExportOutcome {
    Done,
    Cancelled,
    Failed(String),
}

/// Columns of the mix's waveform gathered while rendering.
pub const WAVEFORM_COLUMNS: usize = 480;

/// What the render shows of itself as it goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Waveform {
    /// Lowest and highest sample of each column, over all channels. Columns
    /// not rendered yet are silent.
    pub columns: Vec<(f32, f32)>,
    /// Largest absolute sample so far (above 1 clips integer formats).
    pub peak: f32,
}

impl Default for Waveform {
    fn default() -> Self {
        Self {
            columns: vec![(0., 0.); WAVEFORM_COLUMNS],
            peak: 0.,
        }
    }
}

/// Between the export and its thread.
#[derive(Default)]
struct Shared {
    /// Fraction done, as `f32` bits.
    progress: AtomicU32,
    cancel: AtomicBool,
    waveform: Mutex<Waveform>,
}

/// An export running in the background. The file only appears at `path`
/// once complete.
pub struct Export {
    path: PathBuf,
    seconds: f64,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<ExportOutcome>>,
    outcome: Option<ExportOutcome>,
}

impl Export {
    /// Render `span` of `edit` into a WAV file at `path`. `edit` must have
    /// its audio at `config`'s sample rate.
    pub fn start(
        edit: Edit,
        config: EngineConfig,
        span: (BeatPos, BeatPos),
        path: PathBuf,
        format: WavFormat,
    ) -> Self {
        let tempo = &edit.tempo;
        let seconds = tempo.beats_to_seconds(span.1) - tempo.beats_to_seconds(span.0);
        let shared = Arc::new(Shared::default());
        let job = Job {
            edit,
            config,
            span,
            path: path.clone(),
            format,
            shared: shared.clone(),
        };
        let thread = thread::Builder::new()
            .name("export".into())
            .spawn(move || job.run())
            .map_err(|e| e.to_string());
        let (thread, outcome) = match thread {
            Ok(thread) => (Some(thread), None),
            Err(e) => (None, Some(ExportOutcome::Failed(e))),
        };
        Self {
            path,
            seconds,
            shared,
            thread,
            outcome,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Length of the exported audio, in seconds.
    pub fn seconds(&self) -> f64 {
        self.seconds
    }

    /// Fraction done, 0 to 1.
    pub fn progress(&self) -> f32 {
        f32::from_bits(self.shared.progress.load(Ordering::Relaxed))
    }

    /// The mix rendered so far.
    pub fn waveform(&self) -> Waveform {
        self.shared
            .waveform
            .lock()
            .map(|w| w.clone())
            .unwrap_or_default()
    }

    /// Stop rendering; nothing is written.
    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
    }

    /// How it ended, once it has.
    pub fn outcome(&mut self) -> Option<&ExportOutcome> {
        if self.thread.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(thread) = self.thread.take()
        {
            self.outcome = Some(
                thread
                    .join()
                    .unwrap_or_else(|_| ExportOutcome::Failed("the export crashed".into())),
            );
        }
        self.outcome.as_ref()
    }
}

struct Job {
    edit: Edit,
    config: EngineConfig,
    span: (BeatPos, BeatPos),
    path: PathBuf,
    format: WavFormat,
    shared: Arc<Shared>,
}

impl Job {
    fn run(self) -> ExportOutcome {
        // Written next to the destination, then moved over it: a failed or
        // cancelled export leaves no half-written file behind.
        let temporary = self.path.with_extension("wav.exporting");
        let outcome = self.render(&temporary);
        if outcome == ExportOutcome::Done {
            if let Err(e) = fs::rename(&temporary, &self.path) {
                let _ = fs::remove_file(&temporary);
                return ExportOutcome::Failed(e.to_string());
            }
        } else {
            let _ = fs::remove_file(&temporary);
        }
        outcome
    }

    fn render(&self, temporary: &Path) -> ExportOutcome {
        let failed = |e: &dyn std::fmt::Display| ExportOutcome::Failed(e.to_string());
        let rate = self.config.sample_rate;
        let channels = self.config.output_channels;
        let (start, end) = self.span;
        let first = self.edit.tempo.beats_to_samples(start, rate);
        let frames = (self.edit.tempo.beats_to_samples(end, rate) - first).max(0) as usize;

        let (engine, mut processor) = Engine::new(self.config);
        let mut session = match EditSession::new(self.edit.clone(), engine) {
            Ok(session) => session,
            Err(e) => return failed(&e),
        };
        if let Err(e) = session.seek(start).and_then(|()| session.play()) {
            return failed(&e);
        }
        let mut wav = match WavWriter::create_as(temporary, channels, rate, self.format) {
            Ok(wav) => wav,
            Err(e) => return failed(&e),
        };
        let mut buffer = vec![0.; CHUNK_FRAMES * channels];
        let mut waveform = Waveform::default();
        let mut done = 0;
        while done < frames {
            if self.shared.cancel.load(Ordering::Relaxed) {
                return ExportOutcome::Cancelled;
            }
            let n = CHUNK_FRAMES.min(frames - done);
            let out = &mut buffer[..n * channels];
            out.fill(0.);
            processor.process_interleaved(out, channels);
            if let Err(e) = wav.write_interleaved(out) {
                return failed(&e);
            }
            for (i, frame) in out.chunks_exact(channels).enumerate() {
                let column = (done + i) * WAVEFORM_COLUMNS / frames;
                let (low, high) = &mut waveform.columns[column];
                for &s in frame {
                    *low = low.min(s);
                    *high = high.max(s);
                    waveform.peak = waveform.peak.max(s.abs());
                }
            }
            done += n;
            if let Ok(mut shared) = self.shared.waveform.lock() {
                shared.clone_from(&waveform);
            }
            let fraction = done as f32 / frames as f32;
            self.shared
                .progress
                .store(fraction.to_bits(), Ordering::Relaxed);
        }
        match wav.finalize() {
            Ok(()) => ExportOutcome::Done,
            Err(e) => failed(&e),
        }
    }
}
