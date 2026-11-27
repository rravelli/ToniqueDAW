// Release builds on Windows are GUI apps: no console window behind them.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use crate::ui::run;
use tonique_core::{config::settings::Settings, device::host::Startup, project::ProjectState};

mod ui;

fn main() {
    let state = ProjectState::started(Startup::open(Settings::load()));
    // Ui thread (main thread). Opens the app window
    if let Err(e) = run(state) {
        eprintln!("Couldn't open the window: {e}");
        std::process::exit(1);
    }
}
