//! Rendering the project to an audio file, on a background thread with its
//! own engine, so playback and editing go on meanwhile.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
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

/// An export running in the background. The file only appears at `path`
/// once complete.
pub struct Export {
    path: PathBuf,
    /// Fraction done, as `f32` bits.
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
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
        let progress = Arc::new(AtomicU32::new(0f32.to_bits()));
        let cancel = Arc::new(AtomicBool::new(false));
        let job = Job {
            edit,
            config,
            span,
            path: path.clone(),
            format,
            progress: progress.clone(),
            cancel: cancel.clone(),
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
            progress,
            cancel,
            thread,
            outcome,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Fraction done, 0 to 1.
    pub fn progress(&self) -> f32 {
        f32::from_bits(self.progress.load(Ordering::Relaxed))
    }

    /// Stop rendering; nothing is written.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
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
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
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
        let mut done = 0;
        while done < frames {
            if self.cancel.load(Ordering::Relaxed) {
                return ExportOutcome::Cancelled;
            }
            let n = CHUNK_FRAMES.min(frames - done);
            let out = &mut buffer[..n * channels];
            out.fill(0.);
            processor.process_interleaved(out, channels);
            if let Err(e) = wav.write_interleaved(out) {
                return failed(&e);
            }
            done += n;
            let fraction = done as f32 / frames as f32;
            self.progress.store(fraction.to_bits(), Ordering::Relaxed);
        }
        match wav.finalize() {
            Ok(()) => ExportOutcome::Done,
            Err(e) => failed(&e),
        }
    }
}
