// Release builds on Windows are GUI apps: no console window behind them.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use crate::{audio::host::Startup, config::settings::Settings, core::state::ProjectState, ui::run};

mod ui;

// The core, in its own crate, at the paths it had here.
use tonique_core::{analysis, audio, bench, cache, config, core, file_match, file_tree, utils};
fn main() {
    let state = ProjectState::started(Startup::open(Settings::load()));
    // Ui thread (main thread). Opens the app window
    if let Err(e) = run(state) {
        eprintln!("Couldn't open the window: {e}");
        std::process::exit(1);
    }
}
