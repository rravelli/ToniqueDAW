//! The inspector's Audio panel: how the clip sounds (gain, pan, its
//! transposition), and how it follows the project's tempo (its warp mode,
//! how it's stretched, its file's tempo).

use egui::{Color32, FontId, Rect, Ui, UiBuilder, pos2, vec2};
use tonique_engine::edit::{MAX_TRANSPOSE, StretchSettings, StretchWindow, WarpMode};

use crate::{
    core::{
        clip::{AudioClip, MAX_DETUNE, MIN_DETUNE, carried},
        state::{ClipParam, ProjectState},
    },
    ui::{
        theme::font_size,
        widget::{
            flat_button::FlatButton,
            param_slider::{ParamSlider, SliderEdit},
            select::Select,
        },
    },
};

use super::inspector::{
    CELL_GAP, CLIP_GAIN_RANGE, FIELD_HEIGHT, apply_param, cell_label, section, toggle_section,
};

/// Semitones Shift and an arrow key transpose by.
const OCTAVE: f32 = 12.;
/// Cents Shift and an arrow key detune by.
const DETUNE_BIG_STEP: f32 = 10.;
/// The warp mode's picker, on its section's title row.
const WARP_MODE_WIDTH: f32 = 80.;
/// The file's tempo halved or doubled, beside it.
const TEMPO_BUTTON_WIDTH: f32 = 26.;
/// The file's tempo as typed: `120.00` and room around it.
const TEMPO_FIELD_WIDTH: f32 = 72.;
/// A file's tempo, as typed.
const BPM_RANGE: std::ops::RangeInclusive<f64> = 10.0..=999.0;

/// Gain and pan, sliders like a track's heard as they're dragged, and the
/// transposition and detune alike; the warp, switched by its title, its
/// mode beside it; the stretch's window and formants; the file's tempo,
/// typed or halved and doubled. Edits go in `change` (shown until done) or
/// `live`, as in the inspector.
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
    // heard as it moves, one undo step once released. The pitch's also
    // step by the arrow keys once clicked, an octave (or 10 cents) with
    // Shift; the detune going on into the next semitones.
    let slider = |value: ParamSlider, ui: &mut Ui| {
        let size = vec2(ui.available_width(), FIELD_HEIGHT);
        value.field().color(color).size(size).show(ui).1
    };
    let (gain, pan) = ui.columns(2, |cells| {
        cell_label(&mut cells[0], "Gain");
        let gain = ParamSlider::gain(shown.gain, CLIP_GAIN_RANGE).suffix(" dB");
        let gain = slider(gain, &mut cells[0]);
        cell_label(&mut cells[1], "Pan");
        let pan = slider(ParamSlider::pan(shown.pan), &mut cells[1]);
        (gain, pan)
    });
    apply_param(state, shown.id, ClipParam::Gain, gain);
    apply_param(state, shown.id, ClipParam::Pan, pan);

    ui.add_space(CELL_GAP);
    let (transpose, detune) = ui.columns(2, |cells| {
        cell_label(&mut cells[0], "Transpose");
        let range = -MAX_TRANSPOSE..=MAX_TRANSPOSE;
        let transpose = ParamSlider::steps(shown.transpose as f32, range, OCTAVE, "st");
        let transpose = slider(transpose, &mut cells[0]);
        cell_label(&mut cells[1], "Detune");
        let range = MIN_DETUNE..=MAX_DETUNE;
        let detune = ParamSlider::steps(shown.detune, range, DETUNE_BIG_STEP, "ct").carry();
        let detune = slider(detune, &mut cells[1]);
        (transpose, detune)
    });
    if let Some((transpose, done)) = pitch_edit(transpose) {
        let mut edited = shown.clone();
        edited.set_warp(shown.warp, shown.bpm, transpose as i32, shown.detune, bpm);
        *live = Some((edited, done));
    }
    // Stepped past its range, it carries over into the transposition.
    if let Some((detune, done)) = pitch_edit(detune) {
        let (transpose, detune) = carried(shown.transpose, detune);
        let mut edited = shown.clone();
        edited.set_warp(shown.warp, shown.bpm, transpose, detune, bpm);
        *live = Some((edited, done));
    }

    // Off, it shows how it would play, greyed out, as the loop.
    let warped = shown.warp != WarpMode::Off;
    let (header, toggled) = toggle_section(ui, "Warp", warped, WARP_MODE_WIDTH + CELL_GAP);
    // Stretching is the only mode yet.
    let mut mode = WarpMode::Spectral;
    let picker = Rect::from_min_max(
        pos2(header.right() - WARP_MODE_WIDTH, header.top()),
        header.max,
    );
    // On the row already laid out: nothing more is.
    let mut row = ui.new_child(UiBuilder::new().max_rect(picker));
    if !warped {
        row.disable();
    }
    row.add(
        Select::new(("clip-warp", shown.id), &mut mode)
            .option(WarpMode::Spectral, "Tonal")
            .width(WARP_MODE_WIDTH)
            .height(FIELD_HEIGHT),
    );
    if toggled {
        let warp = if warped { WarpMode::Off } else { mode };
        let mut edited = shown.clone();
        edited.set_warp(warp, shown.bpm, shown.transpose, shown.detune, bpm);
        *live = Some((edited, true));
    }
    ui.add_enabled_ui(warped, |ui| {
        let mut stretch = shown.stretch;
        ui.columns(2, |cells| {
            window_ui(&mut cells[0], shown, &mut stretch);
            formants_ui(&mut cells[1], &mut stretch);
        });
        if stretch != shown.stretch {
            let mut edited = shown.clone();
            edited.stretch = stretch;
            *live = Some((edited, true));
        }

        ui.add_space(CELL_GAP);
        if let Some(file_bpm) = tempo_ui(ui, shown).filter(|b| BPM_RANGE.contains(b)) {
            let mut edited = shown.clone();
            edited.set_warp(shown.warp, file_bpm, shown.transpose, shown.detune, bpm);
            *change = Some((edited, true));
        }
    });
}

/// A pitch slider's edit: its new value, and whether it's done.
fn pitch_edit(edit: SliderEdit) -> Option<(f32, bool)> {
    match edit {
        SliderEdit::None => None,
        SliderEdit::Moved(value) => Some((value, false)),
        SliderEdit::Released { value, .. } => Some((value, true)),
        SliderEdit::Set { to, .. } => Some((to, true)),
    }
}

/// The stretch's window, picked into `stretch`.
fn window_ui(ui: &mut Ui, shown: &AudioClip, stretch: &mut StretchSettings) {
    cell_label(ui, "Window");
    let width = ui.available_width();
    let mut select = Select::new(("clip-window", shown.id), &mut stretch.window)
        .width(width)
        .height(FIELD_HEIGHT);
    for window in StretchWindow::ALL {
        select = select.option(window, window_name(window));
    }
    ui.add(select);
}

/// The stretch's formants, kept or moved with the pitch, into `stretch`.
fn formants_ui(ui: &mut Ui, stretch: &mut StretchSettings) {
    cell_label(ui, "Formants");
    // In the window picker's font, beside it.
    let size = vec2(ui.available_width(), FIELD_HEIGHT);
    let on = stretch.formants;
    let toggle = FlatButton::new(if on { "Keep" } else { "Move" })
        .size(size)
        .font(FontId::proportional(font_size::BODY))
        .selected(on);
    if ui.add(toggle).clicked() {
        stretch.formants = !on;
    }
}

fn window_name(window: StretchWindow) -> &'static str {
    match window {
        StretchWindow::Short => "Short",
        StretchWindow::Normal => "Normal",
        StretchWindow::Long => "Long",
    }
}

/// The file's tempo, typed and applied on Enter (each change resizes the
/// clip), or halved and doubled. Returns the new tempo, if any.
fn tempo_ui(ui: &mut Ui, shown: &AudioClip) -> Option<f64> {
    cell_label(ui, "BPM");
    ui.horizontal(|ui| {
        let field = TEMPO_FIELD_WIDTH;
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

/// `120.00`, `97.25`: two decimals.
fn format_number(value: f64) -> String {
    format!("{value:.2}")
}
