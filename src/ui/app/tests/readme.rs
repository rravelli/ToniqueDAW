//! The README's images: the demo song (`scripts/make_demo.py`), as the app
//! draws it. Run with `cargo test --bin tonique_daw readme_images -- --ignored`
//! to write them to `images/`, or to `TONIQUE_IMAGES_DIR` (CI keeps them
//! apart from the folder's other images).

use super::new_app;
use crate::{
    config::{config_dir, settings::Settings},
    ui::{app::ToniqueApp, project::ProjectAction},
};
use egui::{Event, Vec2, vec2};
use egui_kittest::{Harness, kittest::Queryable};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
use tonique_engine::time::BeatPos;

/// The window, in points: at [`PIXELS_PER_POINT`], the images are
/// 1625 × 1013 pixels.
const SIZE: Vec2 = vec2(1300., 810.);
const PIXELS_PER_POINT: f32 = 1.25;
/// The song's first bars, fit in the timeline: all of it and a little room.
const SHOWN_BEATS: f64 = 30. * 4.;
/// Decoding the stems takes a moment; longer, something's wrong.
const AUDIO_TIMEOUT: Duration = Duration::from_secs(60);

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Where the images go: `images/`, unless `TONIQUE_IMAGES_DIR` says.
fn images_dir() -> PathBuf {
    std::env::var_os("TONIQUE_IMAGES_DIR").map_or_else(|| repo().join("images"), PathBuf::from)
}

/// The demo song, made afresh in the test's own folder.
fn demo_song() -> PathBuf {
    let dir = config_dir().unwrap().join("Demo Song");
    let status = Command::new("python3")
        .arg(repo().join("scripts/make_demo.py"))
        .arg(&dir)
        .status()
        .expect("python3 runs");
    assert!(status.success(), "make_demo.py failed");
    dir.join("Demo Song.tonique")
}

/// The app with `project` open, in `theme`, once its audio is decoded.
fn open(project: &Path, theme: &str) -> Harness<'static, ToniqueApp> {
    let settings = Settings {
        theme: theme.to_owned(),
        ..Settings::default()
    };
    let folder = project.parent().unwrap().to_owned();
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(PIXELS_PER_POINT)
        .build_eframe(move |cc| new_app(cc, &folder, settings));
    let app = harness.state_mut();
    // As it looks with a device: no warning that there's none.
    app.state.ignore_missing_audio();
    app.project.request(
        ProjectAction::OpenRecent(project.to_owned()),
        &mut app.state,
    );
    harness.run_steps(4);
    wait_for_audio(&mut harness);

    let timeline = harness.state().central_panel.timeline_rect();
    assert!(timeline.width() > 0., "the timeline is drawn");
    let grid = &mut harness.state_mut().state.grid;
    grid.fit(BeatPos(0.), BeatPos(SHOWN_BEATS), timeline.width());
    grid.offset.y = 0.;
    harness.run_steps(4);
    harness
}

/// Until every clip's file is decoded: drawn before, they'd have no
/// waveform.
fn wait_for_audio(harness: &mut Harness<'_, ToniqueApp>) {
    let start = Instant::now();
    loop {
        let pending: Vec<String> = harness
            .state()
            .state
            .tracks()
            .flat_map(|track| track.clips)
            .filter(|clip| !clip.audio.data.is_ready())
            .map(|clip| clip.audio.name.clone())
            .collect();
        if pending.is_empty() {
            return;
        }
        assert!(
            start.elapsed() < AUDIO_TIMEOUT,
            "still decoding after {AUDIO_TIMEOUT:?}: {pending:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
        harness.run_steps(1);
    }
}

#[test]
#[ignore = "renders with the GPU and writes images/; run with --ignored"]
fn readme_images() {
    let song = demo_song();

    // The arrangement, with the effects in the browser.
    let mut harness = open(&song, "Dark");
    harness.get_by_label("Effects").click();
    save(&mut harness, "screenshot");
}

/// Saved as `{name}.png` in [`images_dir`], once the pointer has left:
/// kittest draws it, and what it hovers.
fn save(harness: &mut Harness<'_, ToniqueApp>, name: &str) {
    harness.event(Event::PointerGone);
    harness.run_steps(4);
    let image = harness.render().expect("the app renders");
    let dir = images_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.png"));
    image.save(&path).unwrap();
    eprintln!("Wrote {}", path.display());
}
