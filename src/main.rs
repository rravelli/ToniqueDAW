use egui::{Color32, Stroke, Visuals};
use rtrb::RingBuffer;

use crate::message::{GuiToPlayerMsg, ProcessToGuiMsg};

mod analysis;
mod audio;
mod cache;
mod components;
mod config;
mod message;
mod metrics;
mod output;
mod ui;
mod waveform;

fn main() {
    let (to_gui_tx, from_process_rx) = RingBuffer::<ProcessToGuiMsg>::new(256);
    let (to_process_tx, from_gui_rx) = RingBuffer::<GuiToPlayerMsg>::new(64);

    let _cpal_stream = output::spawn_cpal_stream(to_gui_tx, from_gui_rx);

    eframe::run_native(
        "Tonique",
        eframe::NativeOptions::default(),
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();
            egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
            egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Fill);
            cc.egui_ctx.set_fonts(fonts);
            cc.egui_ctx.set_visuals(Visuals {
                window_corner_radius: 1.into(),
                menu_corner_radius: 1.into(),
                extreme_bg_color: Color32::from_gray(80),
                window_stroke: Stroke::NONE,
                ..Visuals::default()
            });
            Ok(Box::new(ui::ToniqueApp::new(
                to_process_tx,
                from_process_rx,
                cc,
            )))
        }),
    )
    .unwrap();
}
