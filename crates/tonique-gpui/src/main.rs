//! Tonique's timeline and file browser rebuilt with gpui-fast, on Tonique's
//! own core, engine and file tree, to compare with the egui UI.

// Tonique's core, without its UI, at the paths it has in Tonique.
use tonique_core::{analysis, audio, bench, cache, config, core, file_tree, utils};

mod app;
mod browser;
mod headers;
mod icons;
mod meter;
mod profile;
mod theme;
mod timeline;
mod wave;
mod widgets;

use gpui::{App, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};

use crate::{
    audio::host::{engine_without_audio, start_audio},
    config::settings::Settings,
};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    // As Tonique's `main`: the chosen device, else the default one, else
    // no audio (the app still opens and keeps trying).
    let mut settings = Settings::load();
    let started = start_audio(&settings).or_else(|e| {
        eprintln!("Audio settings failed ({e}), using the default device");
        settings = Settings {
            ui_scale: settings.ui_scale,
            metronome_level: settings.metronome_level,
            ..Settings::default()
        };
        start_audio(&settings)
    });
    let (audio, engine) = match started {
        Ok((audio, engine)) => (Ok(audio), engine),
        Err(e) => {
            eprintln!("No audio output: {e}");
            (Err(e.to_string()), engine_without_audio(&settings))
        }
    };

    gpui_platform::application().run(move |cx: &mut App| {
        theme::load_fonts(cx);
        let bounds = Bounds::centered(None, size(px(1400.), px(820.)), cx);
        cx.open_window(
            WindowOptions {
                // Benched UIs are compared at the same size: the whole screen.
                window_bounds: Some(if bench::Bench::requested() {
                    WindowBounds::Fullscreen(bounds)
                } else {
                    WindowBounds::Windowed(bounds)
                }),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("Tonique — gpui-fast".into()),
                    ..Default::default()
                }),
                focus: true,
                // gpui-fast caps frames to ~30 fps in low-power mode (e.g. the
                // `power-saver` profile) and while inactive; the egui app
                // doesn't, so a bench lifts the caps to compare like for like.
                ..if bench::Bench::requested() {
                    WindowOptions {
                        low_power_frame_interval: None,
                        inactive_frame_interval: None,
                        thermal_frame_interval: None,
                        ..Default::default()
                    }
                } else {
                    Default::default()
                }
            },
            move |window, cx| cx.new(|cx| app::Tonique::new(engine, audio, settings, window, cx)),
        )
        .expect("the window opens");
        cx.on_window_closed(|cx, _| cx.quit()).detach();
    });
}
