use crate::{
    audio::midi::spawn_midi_thread,
    core::message::{GuiToPlayerMsg, ProcessToGuiMsg},
    ui::font::get_fonts,
};
use eframe::NativeOptions;
use egui::{Color32, Stroke, Visuals};
use rtrb::RingBuffer;
use std::sync::Arc;

mod analysis;
mod audio;
mod cache;
mod config;
mod core;
mod output;
mod ui;
mod ui_main;
pub mod utils;
mod waveform;
fn main() {
    // Create channels
    let (to_gui_tx, from_process_rx) = RingBuffer::<ProcessToGuiMsg>::new(256);
    let (to_process_tx, from_gui_rx) = RingBuffer::<GuiToPlayerMsg>::new(256);
    let (midi_tx, midi_rx) = RingBuffer::<Vec<u8>>::new(256);

    spawn_midi_thread(midi_tx);
    let _cpal_stream = output::spawn_cpal_stream(to_gui_tx, from_gui_rx, midi_rx);
    let mut options = NativeOptions::default();
    let d = eframe::icon_data::from_png_bytes(include_bytes!("../images/logo.png"))
        .expect("The icon data must be valid");
    options.viewport.icon = Some(Arc::new(d));
    eframe::run_native(
        "Tonique",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_fonts(get_fonts());
            cc.egui_ctx.set_visuals(Visuals {
                panel_fill: Color32::from_gray(40),
                window_corner_radius: 1.into(),
                menu_corner_radius: 1.into(),
                extreme_bg_color: Color32::from_gray(80),
                window_stroke: Stroke::NONE,
                ..Visuals::default()
            });
            Ok(Box::new(ui_main::ToniqueApp::new(
                to_process_tx,
                from_process_rx,
                cc,
            )))
        }),
    )
    .unwrap();
}
