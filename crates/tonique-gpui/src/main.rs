//! Tonique's timeline and file browser rebuilt with gpui-fast, on Tonique's
//! own core, engine and file tree, to compare with the egui UI.

// Tonique's core, without its UI, at the paths it has in Tonique.
use tonique_core::{analysis, audio, bench, cache, config, core, file_match, file_tree, utils};

mod app;
mod bottom;
mod browser;
mod geometry;
mod headers;
mod icons;
mod meter;
mod profile;
mod smooth_scroll;
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
    let started = start_audio(&settings).or_else(|error| {
        eprintln!("Audio settings failed ({error}), using the default device");
        settings = Settings {
            ui_scale: settings.ui_scale,
            metronome_level: settings.metronome_level,
            ..Settings::default()
        };
        start_audio(&settings)
    });
    let (audio, engine) = match started {
        Ok((audio, engine)) => (Ok(audio), engine),
        Err(error) => {
            eprintln!("No audio output: {error}");
            (Err(error.to_string()), engine_without_audio(&settings))
        }
    };

    gpui_platform::application().run(move |context: &mut App| {
        theme::load_fonts(context);
        let bounds = Bounds::centered(None, size(px(1400.), px(820.)), context);
        context
            .open_window(
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
                    // Matches the desktop entry, so the window shows as Tonique's.
                    app_id: Some("Tonique".into()),
                    // gpui-fast caps frames in low-power mode and while inactive;
                    // the egui app doesn't, so a bench lifts the caps.
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
                move |window, context| {
                    context
                        .new(|context| app::Tonique::new(engine, audio, settings, window, context))
                },
            )
            .expect("the window opens");
        context
            .on_window_closed(|context, _| context.quit())
            .detach();
    });
}
