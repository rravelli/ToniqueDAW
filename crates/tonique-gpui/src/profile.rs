//! `TONIQUE_PROFILE=1`: time spent in parts of each frame, printed every
//! two seconds, to see where a frame's time goes.

use std::{cell::RefCell, collections::BTreeMap, time::Instant};

thread_local! {
    static STATE: RefCell<Option<Profile>> = RefCell::new(
        std::env::var_os("TONIQUE_PROFILE").map(|_| Profile { since: Instant::now(), frames: 0, totals: BTreeMap::new() })
    );
}

struct Profile {
    since: Instant,
    frames: u32,
    totals: BTreeMap<&'static str, f64>,
}

/// Times `measured` under `name`.
pub fn time<Output>(name: &'static str, measured: impl FnOnce() -> Output) -> Output {
    let start = Instant::now();
    let result = measured();
    let milliseconds = start.elapsed().as_secs_f64() * 1000.;
    STATE.with(|cell| {
        if let Some(profile) = cell.borrow_mut().as_mut() {
            *profile.totals.entry(name).or_default() += milliseconds;
        }
    });
    result
}

/// Call once per frame.
pub fn frame() {
    STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        let Some(profile) = state.as_mut() else {
            return;
        };
        profile.frames += 1;
        let elapsed = profile.since.elapsed().as_secs_f64();
        if elapsed >= 2. {
            let per_frame = |milliseconds: f64| milliseconds / profile.frames as f64;
            let parts: Vec<String> = profile
                .totals
                .iter()
                .map(|(part, total)| format!("{part} {:.2}", per_frame(*total)))
                .collect();
            eprintln!(
                "profile: {:.1} fps, {:.1} ms/frame | ms per frame: {}",
                profile.frames as f64 / elapsed,
                1000. * elapsed / profile.frames as f64,
                parts.join(" · ")
            );
            *profile = Profile {
                since: Instant::now(),
                frames: 0,
                totals: BTreeMap::new(),
            };
        }
    });
}
