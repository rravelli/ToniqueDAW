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

/// Times `f` under `name`.
pub fn time<R>(name: &'static str, f: impl FnOnce() -> R) -> R {
    let start = Instant::now();
    let r = f();
    let ms = start.elapsed().as_secs_f64() * 1000.;
    STATE.with(|s| {
        if let Some(p) = s.borrow_mut().as_mut() {
            *p.totals.entry(name).or_default() += ms;
        }
    });
    r
}

/// Call once per frame.
pub fn frame() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let Some(p) = s.as_mut() else { return };
        p.frames += 1;
        let elapsed = p.since.elapsed().as_secs_f64();
        if elapsed >= 2. {
            let per = |ms: f64| ms / p.frames as f64;
            let parts: Vec<String> = p
                .totals
                .iter()
                .map(|(k, v)| format!("{k} {:.2}", per(*v)))
                .collect();
            eprintln!(
                "profile: {:.1} fps, {:.1} ms/frame | ms per frame: {}",
                p.frames as f64 / elapsed,
                1000. * elapsed / p.frames as f64,
                parts.join(" · ")
            );
            *p = Profile {
                since: Instant::now(),
                frames: 0,
                totals: BTreeMap::new(),
            };
        }
    });
}
