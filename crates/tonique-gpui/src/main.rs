//! Tonique's timeline and file browser rebuilt with gpui-fast, on Tonique's
//! own core, engine and file tree, to compare with the egui UI.

mod app;
mod arrangement;
mod bottom;
mod browser;
mod geometry;
mod graph;
mod icons;
mod meter;
mod model;
mod popup;
mod profile;
mod smooth_scroll;
mod theme;
mod wave;
mod widgets;

use gpui::{App, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};

use tonique_core::{audio::host::Startup, config::settings::Settings, state::ProjectState};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let state = ProjectState::started(Startup::open(Settings::load()));

    gpui_platform::application().run(move |context: &mut App| {
        theme::load_fonts(context);
        let bounds = Bounds::centered(None, size(px(1400.), px(820.)), context);
        context
            .open_window(
                WindowOptions {
                    // Benched UIs are compared at the same size: the whole screen.
                    window_bounds: Some(if tonique_core::bench::Bench::requested() {
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
                    ..if tonique_core::bench::Bench::requested() {
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
                    context.new(|context| app::Workspace::new(state, window, context))
                },
            )
            .expect("the window opens");
        context
            .on_window_closed(|context, _| context.quit())
            .detach();
    });
}
