//! The inspector's Warp tab: how the clip follows the project's tempo,
//! its file's tempo, and its transposition.

use egui::{DragValue, Rect, Ui, pos2, vec2};
use tonique_engine::edit::{MAX_TRANSPOSE, WarpMode};

use crate::{core::clip::AudioClip, ui::widget::select::Select};

use super::inspector::{CELL_GAP, FIELD_HEIGHT, cell, cell_label, gesture, section};

/// Width of the warp mode's picker, on the tab's first row.
const WARP_MODE_WIDTH: f32 = 70.;

/// The warp mode, picked on the first row; the file's tempo, typed; the
/// transposition, heard as it's dragged (`live`). Edits go in `change`
/// (shown until done) or `live`, as in the inspector.
pub(super) fn warp_ui(
    ui: &mut Ui,
    shown: &AudioClip,
    bpm: f32,
    change: &mut Option<(AudioClip, bool)>,
    live: &mut Option<(AudioClip, bool)>,
) {
    let header = section(ui, "Mode", WARP_MODE_WIDTH + CELL_GAP);
    let picker = Rect::from_min_max(
        pos2(header.right() - WARP_MODE_WIDTH, header.top()),
        header.max,
    );
    let mut warp = shown.warp;
    ui.put(
        picker,
        Select::new(("clip-warp", shown.id), &mut warp)
            .option(WarpMode::Off, "Off")
            .option(WarpMode::Spectral, "Tonal")
            .width(WARP_MODE_WIDTH),
    )
    .on_hover_text(
        "Off: plays at its own pace, transposing it changes its speed.\n\
         Stretch: follows the project's tempo, transposing moves the pitch alone.",
    );
    if warp != shown.warp {
        let mut edited = shown.clone();
        edited.set_warp(warp, shown.bpm, shown.transpose, shown.detune, bpm);
        *live = Some((edited, true));
    }
    let warped = shown.warp == WarpMode::Spectral;
    ui.columns(3, |cells| {
        // Typed, applied on Enter: each change resizes the clip.
        let typed = cells[0]
            .add_enabled_ui(warped, |ui| {
                let id = ui.id().with(("clip-bpm", shown.id));
                typed_cell(ui, "BPM", id, shown.bpm).1
            })
            .inner;
        if let Some(file_bpm) = typed.filter(|b| (10.0..=999.0).contains(b)) {
            let mut edited = shown.clone();
            edited.set_warp(shown.warp, file_bpm, shown.transpose, shown.detune, bpm);
            *change = Some((edited, true));
        }

        let mut transpose = shown.transpose;
        let limit = MAX_TRANSPOSE as i32;
        let response = cell(
            &mut cells[1],
            "Transpose",
            DragValue::new(&mut transpose)
                .speed(0.1)
                .range(-limit..=limit)
                .custom_formatter(|v, _| format!("{v:+.0} st"))
                .custom_parser(|text| text.trim().trim_end_matches("st").trim().parse().ok()),
        );
        if let Some(done) = gesture(&response) {
            let mut edited = shown.clone();
            edited.set_warp(shown.warp, shown.bpm, transpose, shown.detune, bpm);
            *live = Some((edited, done));
        }

        let mut detune = shown.detune;
        let response = cell(
            &mut cells[2],
            "Detune",
            DragValue::new(&mut detune)
                .speed(0.5)
                .range(-50.0..=50.0)
                .max_decimals(0)
                .custom_formatter(|v, _| format!("{v:+.0} ct"))
                .custom_parser(|text| text.trim().trim_end_matches("ct").trim().parse().ok()),
        );
        if let Some(done) = gesture(&response) {
            let mut edited = shown.clone();
            edited.set_warp(shown.warp, shown.bpm, shown.transpose, detune, bpm);
            *live = Some((edited, done));
        }
    });
}

/// A cell whose number is typed and applied on Enter, not as typed nor
/// dragged: for values costly to apply at each keystroke. Returns the
/// value typed, once entered.
fn typed_cell(ui: &mut Ui, label: &str, id: egui::Id, value: f64) -> (egui::Response, Option<f64>) {
    cell_label(ui, label);
    let shown = format_number(value);
    let mut text = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| shown.clone());
    let response = ui.add_sized(
        vec2(ui.available_width(), FIELD_HEIGHT),
        egui::TextEdit::singleline(&mut text).horizontal_align(egui::Align::Center),
    );
    let entered = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, text.clone()));
    } else {
        // Left without Enter: back to the value.
        ui.data_mut(|d| d.remove::<String>(id));
    }
    let typed = entered.then(|| text.trim().parse::<f64>().ok()).flatten();
    (response, typed)
}

/// `120`, `98.5`, `97.25`: no more decimals than needed, two at most.
fn format_number(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}
