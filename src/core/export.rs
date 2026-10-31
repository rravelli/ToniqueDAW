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

    /// The audio of the file `index` rendered so far.
    pub fn waveform(&self, index: usize) -> Option<Waveform> {
        self.shared.waveforms.lock().ok()?.get(index).cloned()
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

impl Drop for Export {
    /// Cancel and wait (a chunk at most), so the thread removes its
    /// temporaries even when the app quits mid-export.
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
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
            let destinations: Vec<&Path> = self.renders.iter().map(|r| r.path.as_path()).collect();
            if let Err(e) = replace_all(&temporaries, &destinations) {
                outcome = ExportOutcome::Failed(e.to_string());
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

/// Move each of `temporaries` over its destination, all or none: should one
/// move fail, the files already moved are put back as they were.
fn replace_all(temporaries: &[PathBuf], destinations: &[&Path]) -> std::io::Result<()> {
    // Each destination moved, and where the file it replaced was kept.
    let mut moved: Vec<(&Path, Option<PathBuf>)> = Vec::new();
    let mut result = Ok(());
    for (temporary, &destination) in temporaries.iter().zip(destinations) {
        let backup = destination.with_extension("wav.replaced");
        let kept = if destination.exists() {
            if let Err(e) = fs::rename(destination, &backup) {
                result = Err(e);
                break;
            }
            Some(backup)
        } else {
            None
        };
        if let Err(e) = fs::rename(temporary, destination) {
            // Nothing was moved there: only the old file goes back.
            if let Some(kept) = kept {
                let _ = fs::rename(kept, destination);
            }
            result = Err(e);
            break;
        }
        moved.push((destination, kept));
    }
    for (destination, kept) in moved.into_iter().rev() {
        match (&result, kept) {
            (Ok(()), Some(kept)) => {
                let _ = fs::remove_file(kept);
            }
            (Ok(()), None) => {}
            (Err(_), Some(kept)) => {
                let _ = fs::rename(kept, destination);
            }
            (Err(_), None) => {
                let _ = fs::remove_file(destination);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A move that fails puts the files already replaced back as they were.
    #[test]
    fn a_failed_replace_puts_everything_back() {
        let dir = std::env::temp_dir().join(format!("tonique-replace-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (old, new) = (dir.join("old.wav"), dir.join("new.wav"));
        fs::write(&old, "before").unwrap();
        let temporaries = [dir.join("old.wav.exporting"), dir.join("new.wav.exporting")];
        fs::write(&temporaries[0], "after").unwrap();
        // The second temporary is missing, so its move fails.
        assert!(replace_all(&temporaries, &[&old, &new]).is_err());
        assert_eq!(fs::read_to_string(&old).unwrap(), "before");
        assert!(!new.exists());
        assert!(!old.with_extension("wav.replaced").exists());

        for temporary in &temporaries {
            fs::write(temporary, "after").unwrap();
        }
        assert!(replace_all(&temporaries, &[&old, &new]).is_ok());
        assert_eq!(fs::read_to_string(&old).unwrap(), "after");
        assert_eq!(fs::read_to_string(&new).unwrap(), "after");
        assert!(!old.with_extension("wav.replaced").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
