//! Rendering the project to audio files (the mix, or tracks on their own),
//! on a background thread with its own engine, so playback and editing go
//! on meanwhile.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
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
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ExportRange {
    /// From the start of the first clip to the end of the last.
    #[default]
    Project,
    /// The loop region.
    Loop,
    /// From one position to another.
    Custom(BeatPos, BeatPos),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ExportOptions {
    pub range: ExportRange,
    pub format: WavFormat,
    /// Seconds rendered past the end, for reverbs and echoes to ring out.
    pub tail_seconds: u32,
}

/// One file of an export: `edit` rendered into `path`.
pub struct Render {
    /// What the file holds, to show: the mix or a track's name.
    pub label: String,
    pub path: PathBuf,
    pub edit: Edit,
}

/// A file an export writes.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportFile {
    pub label: String,
    pub path: PathBuf,
}

/// How an export ended.
#[derive(Clone, Debug, PartialEq)]
pub enum ExportOutcome {
    Done,
    Cancelled,
    Failed(String),
}

/// Columns of each file's waveform gathered while rendering.
pub const WAVEFORM_COLUMNS: usize = 480;

/// What a render shows of itself as it goes.
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
    /// Fraction of all the files done, as `f32` bits.
    progress: AtomicU32,
    /// The file being rendered.
    current: AtomicUsize,
    cancel: AtomicBool,
    /// One per file.
    waveforms: Mutex<Vec<Waveform>>,
}

/// An export running in the background, one file after another. The files
/// only appear once all of them are complete.
pub struct Export {
    files: Vec<ExportFile>,
    seconds: f64,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<ExportOutcome>>,
    outcome: Option<ExportOutcome>,
}

impl Export {
    /// Render `span` of each edit, plus `tail_seconds`, into a WAV file.
    /// The edits must have their audio at `config`'s sample rate.
    pub fn start(
        renders: Vec<Render>,
        config: EngineConfig,
        span: (BeatPos, BeatPos),
        format: WavFormat,
        tail_seconds: u32,
    ) -> Self {
        let files = renders
            .iter()
            .map(|r| ExportFile {
                label: r.label.clone(),
                path: r.path.clone(),
            })
            .collect();
        let seconds = renders.first().map_or(0., |r| {
            let tempo = &r.edit.tempo;
            tempo.beats_to_seconds(span.1) - tempo.beats_to_seconds(span.0)
        }) + tail_seconds as f64;
        let shared = Arc::new(Shared {
            waveforms: Mutex::new(vec![Waveform::default(); renders.len()]),
            ..Default::default()
        });
        let job = Job {
            renders,
            config,
            span,
            tail_seconds,
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
            files,
            seconds,
            shared,
            thread,
            outcome,
        }
    }

    pub fn files(&self) -> &[ExportFile] {
        &self.files
    }

    /// Length of each exported file, in seconds.
    pub fn seconds(&self) -> f64 {
        self.seconds
    }

    /// Fraction of all the files done, 0 to 1.
    pub fn progress(&self) -> f32 {
        f32::from_bits(self.shared.progress.load(Ordering::Relaxed))
    }

    /// The index of the file being rendered.
    pub fn current(&self) -> usize {
        self.shared.current.load(Ordering::Relaxed)
    }

    /// Each file's audio rendered so far.
    pub fn waveforms(&self) -> Vec<Waveform> {
        self.shared
            .waveforms
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
    renders: Vec<Render>,
    config: EngineConfig,
    span: (BeatPos, BeatPos),
    tail_seconds: u32,
    format: WavFormat,
    shared: Arc<Shared>,
}

impl Job {
    fn run(self) -> ExportOutcome {
        // Written next to their destination, then moved over it once all
        // are rendered: a failed or cancelled export leaves no file behind.
        let temporaries: Vec<PathBuf> = self
            .renders
            .iter()
            .map(|r| r.path.with_extension("wav.exporting"))
            .collect();
        let mut outcome = ExportOutcome::Done;
        for (index, (render, temporary)) in self.renders.iter().zip(&temporaries).enumerate() {
            self.shared.current.store(index, Ordering::Relaxed);
            outcome = self.render(index, &render.edit, temporary);
            if outcome != ExportOutcome::Done {
                break;
            }
        }
        if outcome == ExportOutcome::Done {
            for (render, temporary) in self.renders.iter().zip(&temporaries) {
                if let Err(e) = fs::rename(temporary, &render.path) {
                    outcome = ExportOutcome::Failed(e.to_string());
                    break;
                }
            }
        }
        for temporary in &temporaries {
            let _ = fs::remove_file(temporary);
        }
        outcome
    }

    /// Render the file `index` of the export.
    fn render(&self, index: usize, edit: &Edit, temporary: &Path) -> ExportOutcome {
        let failed = |e: &dyn std::fmt::Display| ExportOutcome::Failed(e.to_string());
        let rate = self.config.sample_rate;
        let channels = self.config.output_channels;
        let (start, end) = self.span;
        let first = edit.tempo.beats_to_samples(start, rate);
        let frames = (edit.tempo.beats_to_samples(end, rate) - first).max(0) as usize
            + (self.tail_seconds as f64 * rate) as usize;

        let (engine, mut processor) = Engine::new(self.config);
        let mut session = match EditSession::new(edit.clone(), engine) {
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
        let files = self.renders.len() as f32;
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
            if let Ok(mut shared) = self.shared.waveforms.lock()
                && let Some(shared) = shared.get_mut(index)
            {
                shared.clone_from(&waveform);
            }
            let fraction = (index as f32 + done as f32 / frames as f32) / files;
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
