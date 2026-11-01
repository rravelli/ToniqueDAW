//! The inspector's Audio panel: how the clip sounds (gain, pan), how it
//! follows the project's tempo (its file's tempo), and its transposition.

use egui::{Color32, Ui, vec2};
use tonique_engine::edit::{MAX_TRANSPOSE, WarpMode};

use crate::{
    core::{
        clip::AudioClip,
        state::{ClipParam, ProjectState},
    },
    ui::widget::{flat_button::FlatButton, knob::Knob, param_slider::ParamSlider},
};

use super::inspector::{
    CELL_GAP, CLIP_GAIN_RANGE, FIELD_HEIGHT, apply_param, cell_label, gesture, section,
    switch_section,
};

/// The file's tempo halved or doubled, beside it.
const TEMPO_BUTTON_WIDTH: f32 = 26.;
/// A file's tempo, as typed.
const BPM_RANGE: std::ops::RangeInclusive<f64> = 10.0..=999.0;

/// Gain and pan, sliders like a track's heard as they're dragged; warping
/// switched on or off; the file's tempo, typed or halved and doubled; the
/// transposition on knobs, heard as they're turned (`live`). Edits go in
/// `change` (shown until done) or `live`, as in the inspector.
pub(super) fn audio_panel(
    ui: &mut Ui,
    shown: &AudioClip,
    color: Color32,
    bpm: f32,
    state: &mut ProjectState,
    change: &mut Option<(AudioClip, bool)>,
    live: &mut Option<(AudioClip, bool)>,
) {
    section(ui, "Audio", 0.);
    // As on tracks: a level in the clip's colour, reset by a double-click;
    // heard as it moves, one undo step once released.
    let (gain, pan) = ui.columns(2, |cells| {
        let size = vec2(cells[0].available_width(), FIELD_HEIGHT);
        cell_label(&mut cells[0], "Gain");
        let gain = ParamSlider::gain(shown.gain, CLIP_GAIN_RANGE)
            .field()
            .color(color)
            .size(size)
            .suffix(" dB")
            .show(&mut cells[0])
            .1;
        cell_label(&mut cells[1], "Pan");
        let pan = ParamSlider::pan(shown.pan)
            .field()
            .color(color)
            .size(size)
            .show(&mut cells[1])
            .1;
        (gain, pan)
    });
    apply_param(state, shown.id, ClipParam::Gain, gain);
    apply_param(state, shown.id, ClipParam::Pan, pan);

    let warped = shown.warp == WarpMode::Spectral;
    if switch_section(ui, "Warp", warped) {
        let warp = if warped {
            WarpMode::Off
        } else {
            WarpMode::Spectral
        };
        let mut edited = shown.clone();
        edited.set_warp(warp, shown.bpm, shown.transpose, shown.detune, bpm);
        *live = Some((edited, true));
    }
    let file_bpm = ui.add_enabled_ui(warped, |ui| tempo_ui(ui, shown)).inner;
    if let Some(file_bpm) = file_bpm.filter(|b| BPM_RANGE.contains(b)) {
        let mut edited = shown.clone();
        edited.set_warp(shown.warp, file_bpm, shown.transpose, shown.detune, bpm);
        *change = Some((edited, true));
    }

    ui.add_space(CELL_GAP);
    ui.columns(2, |cells| {
        let mut transpose = shown.transpose as f32;
        let limit = MAX_TRANSPOSE;
        let response = cells[0].vertical_centered(|ui| {
            ui.add(
                Knob::new(&mut transpose, -limit..=limit)
                    .name("Transpose")
                    .default(Some(0.))
                    .format(&|v| format!("{:+} st", v.round() as i32)),
            )
        });
        if let Some(done) = gesture(&response.inner) {
            let mut edited = shown.clone();
            let transpose = transpose.round() as i32;
            edited.set_warp(shown.warp, shown.bpm, transpose, shown.detune, bpm);
            *live = Some((edited, done));
        }

        let mut detune = shown.detune;
        let response = cells[1].vertical_centered(|ui| {
            ui.add(
                Knob::new(&mut detune, -50.0..=50.)
                    .name("Detune")
                    .default(Some(0.))
                    .format(&|v| format!("{:+.0} ct", v)),
            )
        });
        if let Some(done) = gesture(&response.inner) {
            let mut edited = shown.clone();
            edited.set_warp(shown.warp, shown.bpm, shown.transpose, detune.round(), bpm);
            *live = Some((edited, done));
        }
    });
}

/// The file's tempo, typed and applied on Enter (each change resizes the
/// clip), or halved and doubled. Returns the new tempo, if any.
fn tempo_ui(ui: &mut Ui, shown: &AudioClip) -> Option<f64> {
    cell_label(ui, "BPM");
    ui.horizontal(|ui| {
        let field = ui.available_width() - 2. * (TEMPO_BUTTON_WIDTH + CELL_GAP);
        let id = ui.id().with(("clip-bpm", shown.id));
        let typed = typed_field(ui, id, shown.bpm, field);
        let button = |text| FlatButton::new(text).size(vec2(TEMPO_BUTTON_WIDTH, FIELD_HEIGHT));
        let halved = ui.add(button("÷2")).clicked().then_some(shown.bpm / 2.);
        let doubled = ui.add(button("×2")).clicked().then_some(shown.bpm * 2.);
        typed.or(halved).or(doubled)
    })
    .inner
}

/// A number typed and applied on Enter, not as typed nor dragged: for
/// values costly to apply at each keystroke. Returns the value typed, once
/// entered.
fn typed_field(ui: &mut Ui, id: egui::Id, value: f64, width: f32) -> Option<f64> {
    let shown = format_number(value);
    let mut text = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| shown.clone());
    let response = ui.add_sized(
        vec2(width, FIELD_HEIGHT),
        egui::TextEdit::singleline(&mut text).horizontal_align(egui::Align::Center),
    );
    let entered = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, text.clone()));
    } else {
        // Left without Enter: back to the value.
        ui.data_mut(|d| d.remove::<String>(id));
    }
    entered.then(|| text.trim().parse::<f64>().ok()).flatten()
}

/// `120`, `98.5`, `97.25`: no more decimals than needed, two at most.
fn format_number(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}
