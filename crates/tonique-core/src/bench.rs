//! A UI benchmark, the same for every UI built on this core: started with
//! `TONIQUE_BENCH=<project file>`, it opens the project (scaled up by
//! `TONIQUE_BENCH_SCALE`, 3 by default), waits for its audio, then steers
//! the view through fixed phases (idle, playing, zooming, scrolling) by time,
//! so every UI shows the same thing at the same moment. Each frame, the UI
//! calls [`Bench::frame`] before drawing.
//!
//! Measured per frame: the UI thread's CPU time since the last frame (all it
//! did for that frame: layout, painting, handing it to the GPU; not waiting
//! for vsync) and the wall time between frames. Per phase: the process's
//! CPU use. At the end: its memory. The report is printed and written as
//! JSON to `TONIQUE_BENCH_OUT` (`bench-<ui>.json` by default), then the
//! app exits.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use tonique_engine::time::BeatPos;

use crate::core::{
    grid::Offset,
    project::ProjectFile,
    state::{MASTER_TRACK_ID, PlaybackState, ProjectState},
};

/// Longest wait for the project's audio to decode.
const LOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// Frames drawn after loading, before measuring.
const WARMUP: Duration = Duration::from_secs(2);
/// Width the arrangement is laid out for, in points: the zoom levels are
/// fixed from it, so every UI shows the same beats whatever its panels.
const VIEW_WIDTH: f32 = 1400.;
/// Height the rows are scrolled through.
const VIEW_HEIGHT: f32 = 600.;
/// Deepest zoom, in points per beat (a few samples per point at 120 BPM).
const MAX_ZOOM: f32 = 2000.;
/// Zoom while scrolling across.
const SCROLL_ZOOM: f32 = 60.;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Idle,
    Play,
    Zoom,
    ScrollX,
    ScrollY,
}

impl Phase {
    const ALL: [(Phase, f32); 5] = [
        (Phase::Idle, 5.),
        (Phase::Play, 10.),
        (Phase::Zoom, 10.),
        (Phase::ScrollX, 8.),
        (Phase::ScrollY, 6.),
    ];

    fn name(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Play => "play",
            Phase::Zoom => "zoom",
            Phase::ScrollX => "scroll-x",
            Phase::ScrollY => "scroll-y",
        }
    }

    /// The phase at `t` seconds into the run, and how far into it (0 to 1).
    fn at(t: f32) -> Option<(Phase, f32)> {
        let mut start = 0.;
        for (phase, length) in Self::ALL {
            if t < start + length {
                return Some((phase, (t - start) / length));
            }
            start += length;
        }
        None
    }
}

enum Stage {
    Loading {
        since: Instant,
    },
    Warmup {
        since: Instant,
    },
    Running {
        since: Instant,
        phase: Option<Phase>,
    },
}

struct PhaseRecord {
    phase: Phase,
    /// UI thread CPU per frame, ms.
    cpu: Vec<f64>,
    /// Between frames, ms.
    wall: Vec<f64>,
    started: Instant,
    process_cpu_at_start: Option<f64>,
    elapsed: f64,
    process_cpu: Option<f64>,
}

pub struct Bench {
    ui: String,
    project: PathBuf,
    scale: usize,
    out: PathBuf,
    stage: Stage,
    last: Option<(Instant, f64)>,
    phases: Vec<PhaseRecord>,
    /// Beats the scaled project spans.
    length: f64,
    tracks: usize,
    clips: usize,
    rows_height: f32,
}

impl Bench {
    /// A bench for UI `ui`, if `TONIQUE_BENCH` names a project.
    pub fn from_env(ui: &str) -> Option<Self> {
        let project = PathBuf::from(std::env::var_os("TONIQUE_BENCH")?);
        let scale = std::env::var("TONIQUE_BENCH_SCALE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3usize)
            .max(1);
        let out = std::env::var_os("TONIQUE_BENCH_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("bench-{ui}.json")));
        Some(Self {
            ui: ui.to_string(),
            project,
            scale,
            out,
            stage: Stage::Loading {
                since: Instant::now(),
            },
            last: None,
            phases: Vec::new(),
            length: 0.,
            tracks: 0,
            clips: 0,
            rows_height: 0.,
        })
    }

    /// Whether a bench was asked for: the UI opens fullscreen then.
    pub fn requested() -> bool {
        std::env::var_os("TONIQUE_BENCH").is_some()
    }

    /// Open the project, scaled up, silenced. Call once, on the state the
    /// UI will draw.
    pub fn setup(&mut self, state: &mut ProjectState) {
        let project = match ProjectFile::read(&self.project) {
            Ok(project) => project,
            Err(e) => fail(&format!("can't open {}: {e}", self.project.display())),
        };
        let dir = self
            .project
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();
        for problem in state.load_project(&project, &dir) {
            eprintln!("bench: {problem}");
        }
        // More tracks, then each one longer.
        let originals: Vec<_> = state.tracks().map(|t| t.id).collect();
        for _ in 1..self.scale {
            for id in &originals {
                state.duplicate_track(id);
            }
        }
        for _ in 1..self.scale {
            state.select_all_clips();
            state.duplicate_selected_clips();
        }
        state.clear_clip_selection();
        state.set_mute(MASTER_TRACK_ID, true);
        self.tracks = state.track_count();
        self.clips = state.tracks().map(|t| t.clips.len()).sum();
        self.length = state.arrangement_end().0.max(4.);
        self.rows_height = state.tracks().map(|t| t.height + 1.).sum();
        eprintln!(
            "bench: {} scaled ×{}: {} tracks, {} clips, {:.0} beats; loading audio…",
            self.project.display(),
            self.scale,
            self.tracks,
            self.clips,
            self.length
        );
    }

    /// Call at the start of each frame, before drawing: records the last
    /// frame and sets the view for this one. Exits the app once done.
    pub fn frame(&mut self, state: &mut ProjectState) {
        let now = Instant::now();
        let cpu = thread_cpu_ms();
        let last = self.last.replace((now, cpu));
        match self.stage {
            Stage::Loading { since } => {
                let ready = state
                    .tracks()
                    .flat_map(|t| t.clips)
                    .all(|c| c.audio.missing || c.audio.data.is_ready());
                if ready || since.elapsed() > LOAD_TIMEOUT {
                    if !ready {
                        eprintln!("bench: audio still loading, starting anyway");
                    }
                    self.stage = Stage::Warmup { since: now };
                }
                self.view_overview(state);
            }
            Stage::Warmup { since } => {
                self.view_overview(state);
                if since.elapsed() > WARMUP {
                    self.stage = Stage::Running {
                        since: now,
                        phase: None,
                    };
                }
            }
            Stage::Running { since, phase } => {
                // The frame that just ended belongs to the phase it ran in.
                if let (Some(phase), Some((then, cpu_then))) = (phase, last)
                    && let Some(record) = self.phases.last_mut()
                    && record.phase == phase
                {
                    record.cpu.push(cpu - cpu_then);
                    record
                        .wall
                        .push(now.duration_since(then).as_secs_f64() * 1000.);
                }
                let t = now.duration_since(since).as_secs_f32();
                let Some((current, progress)) = Phase::at(t) else {
                    self.finish(state, now);
                };
                if phase != Some(current) {
                    self.end_phase(now);
                    self.start_phase(state, current, now);
                    self.stage = Stage::Running {
                        since,
                        phase: Some(current),
                    };
                }
                self.steer(state, current, progress);
            }
        }
    }

    fn view_overview(&self, state: &mut ProjectState) {
        state
            .grid
            .fit(BeatPos::ZERO, BeatPos(self.length), VIEW_WIDTH);
        state.grid.offset = Offset::ZERO;
    }

    fn start_phase(&mut self, state: &mut ProjectState, phase: Phase, now: Instant) {
        self.view_overview(state);
        if phase == Phase::Play {
            state.seek(BeatPos::ZERO);
            state.play();
        }
        self.phases.push(PhaseRecord {
            phase,
            cpu: Vec::new(),
            wall: Vec::new(),
            started: now,
            process_cpu_at_start: process_cpu_seconds(),
            elapsed: 0.,
            process_cpu: None,
        });
    }

    fn end_phase(&mut self, now: Instant) {
        if let Some(record) = self.phases.last_mut() {
            record.elapsed = now.duration_since(record.started).as_secs_f64();
            record.process_cpu = process_cpu_seconds()
                .zip(record.process_cpu_at_start)
                .map(|(end, start)| 100. * (end - start) / record.elapsed.max(1e-9));
        }
    }

    /// The view `progress` (0 to 1) into `phase`.
    fn steer(&self, state: &mut ProjectState, phase: Phase, progress: f32) {
        let fit = VIEW_WIDTH / self.length as f32;
        let grid = &mut state.grid;
        match phase {
            Phase::Idle | Phase::Play => {}
            Phase::Zoom => {
                // In to the deepest zoom and back out, around the middle.
                let depth = 1. - (2. * progress - 1.).abs();
                let zoom = fit * (MAX_ZOOM / fit).max(1.).powf(depth);
                let middle = (self.length / 2.) as f32;
                grid.fit(
                    BeatPos::ZERO,
                    BeatPos((VIEW_WIDTH / zoom) as f64),
                    VIEW_WIDTH,
                );
                grid.offset.x = (middle * zoom - VIEW_WIDTH / 2.).max(0.);
            }
            Phase::ScrollX => {
                let zoom = SCROLL_ZOOM.max(fit);
                grid.fit(
                    BeatPos::ZERO,
                    BeatPos((VIEW_WIDTH / zoom) as f64),
                    VIEW_WIDTH,
                );
                let span = (self.length as f32 * zoom - VIEW_WIDTH).max(0.);
                // Across and back.
                grid.offset.x = span * (1. - (2. * progress - 1.).abs());
            }
            Phase::ScrollY => {
                let span = (self.rows_height - VIEW_HEIGHT).max(0.);
                grid.offset.y = span * (1. - (2. * progress - 1.).abs());
            }
        }
    }

    fn finish(&mut self, state: &mut ProjectState, now: Instant) -> ! {
        self.end_phase(now);
        if state.playback_state() == PlaybackState::Playing {
            state.stop();
        }
        let report = self.report();
        eprintln!("{}", report.0);
        if let Err(e) = std::fs::write(&self.out, report.1) {
            eprintln!("bench: can't write {}: {e}", self.out.display());
        } else {
            eprintln!("bench: written to {}", self.out.display());
        }
        std::process::exit(0);
    }

    /// The table, and the JSON.
    fn report(&self) -> (String, String) {
        let mut table = format!(
            "\n{} — {} tracks, {} clips\n{:<9} {:>6} {:>6} {:>8} {:>8} {:>8} {:>8} {:>7} {:>6}\n",
            self.ui,
            self.tracks,
            self.clips,
            "phase",
            "frames",
            "fps",
            "cpu p50",
            "cpu p95",
            "cpu p99",
            "cpu max",
            "dropped",
            "proc%"
        );
        let mut phases = Vec::new();
        for r in &self.phases {
            let fps = r.wall.len() as f64 / r.elapsed.max(1e-9);
            let median_wall = percentile(&r.wall, 50.);
            // Frames that took over 1.5 times the usual interval.
            let dropped = r.wall.iter().filter(|w| **w > 1.5 * median_wall).count();
            let p = |q| percentile(&r.cpu, q);
            table += &format!(
                "{:<9} {:>6} {:>6.1} {:>6.2}ms {:>6.2}ms {:>6.2}ms {:>6.2}ms {:>7} {:>5.0}%\n",
                r.phase.name(),
                r.cpu.len(),
                fps,
                p(50.),
                p(95.),
                p(99.),
                p(100.),
                dropped,
                r.process_cpu.unwrap_or(f64::NAN),
            );
            phases.push(serde_json::json!({
                "phase": r.phase.name(),
                "frames": r.cpu.len(),
                "seconds": r.elapsed,
                "fps": fps,
                "cpu_ms": { "p50": p(50.), "p95": p(95.), "p99": p(99.), "max": p(100.), "mean": mean(&r.cpu) },
                "frame_interval_ms": { "p50": median_wall, "p99": percentile(&r.wall, 99.) },
                "dropped": dropped,
                "process_cpu_percent": r.process_cpu,
            }));
        }
        let memory = memory_kb();
        if let Some((rss, peak)) = memory {
            table += &format!("memory: {} MB now, {} MB peak\n", rss / 1024, peak / 1024);
        }
        let json = serde_json::json!({
            "ui": self.ui,
            "project": self.project,
            "scale": self.scale,
            "tracks": self.tracks,
            "clips": self.clips,
            "beats": self.length,
            "phases": phases,
            "rss_mb": memory.map(|m| m.0 / 1024),
            "peak_rss_mb": memory.map(|m| m.1 / 1024),
        });
        (
            table,
            serde_json::to_string_pretty(&json).unwrap_or_default(),
        )
    }
}

fn fail(message: &str) -> ! {
    eprintln!("bench: {message}");
    std::process::exit(1);
}

fn percentile(values: &[f64], q: f64) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let i = ((q / 100.) * (sorted.len() - 1) as f64).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len().max(1) as f64
}

/// CPU time of the calling thread, in ms; not measured off Unix.
#[cfg(not(unix))]
fn thread_cpu_ms() -> f64 {
    f64::NAN
}

/// CPU time of the calling thread, in ms.
#[cfg(unix)]
fn thread_cpu_ms() -> f64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid timespec for the call to fill.
    let ok = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) } == 0;
    if ok {
        ts.tv_sec as f64 * 1000. + ts.tv_nsec as f64 / 1e6
    } else {
        f64::NAN
    }
}

/// CPU time of the whole process (all threads), in seconds; only read
/// from Linux's `/proc`.
#[cfg(not(unix))]
fn process_cpu_seconds() -> Option<f64> {
    None
}

/// CPU time of the whole process (all threads), in seconds.
#[cfg(unix)]
fn process_cpu_seconds() -> Option<f64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    // After the command name, which may hold spaces, in parentheses.
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    let ticks = fields.get(11)?.parse::<f64>().ok()? + fields.get(12)?.parse::<f64>().ok()?;
    // SAFETY: sysconf has no preconditions.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    Some(ticks / hz)
}

/// Resident memory now and at its peak, in kB.
fn memory_kb() -> Option<(u64, u64)> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let field = |name: &str| {
        status
            .lines()
            .find(|l| l.starts_with(name))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    };
    Some((field("VmRSS:")?, field("VmHWM:")?))
}
