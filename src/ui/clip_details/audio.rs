//! The inspector's Audio panel: how the clip sounds (gain, pan), how it
//! follows the project's tempo (its file's tempo, how it's stretched), and
//! its transposition.

use egui::{Color32, DragValue, Rect, Shape, Ui, pos2, vec2};
use tonique_engine::edit::{MAX_TRANSPOSE, StretchSettings, StretchWindow, WarpMode};

use crate::{
    core::{
        clip::AudioClip,
        state::{ClipParam, ProjectState},
    },
    ui::widget::{
        flat_button::FlatButton,
        param_slider::{FIELD_LEVEL, ParamSlider},
        select::{self, Select},
    },
};

use super::inspector::{
    CELL_GAP, CLIP_GAIN_RANGE, FIELD_HEIGHT, apply_param, cell_label, gesture, section, section_row,
};

/// The warp mode's picker, on its section's title row.
const WARP_MODE_WIDTH: f32 = 80.;
/// The file's tempo halved or doubled, beside it.
const TEMPO_BUTTON_WIDTH: f32 = 22.;
/// Fine tuning's range either way, in cents.
const MAX_DETUNE: f32 = 50.;
/// A file's tempo, as typed.
const BPM_RANGE: std::ops::RangeInclusive<f64> = 10.0..=999.0;

/// Gain and pan, sliders like a track's heard as they're dragged; the warp
/// mode; the file's tempo, typed or halved and doubled, beside the
/// stretch's window; the transposition, heard as it's dragged (`live`)
/// and filled in the clip's `color`, beside the formants. Edits go in
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

    // Its mode picked on the title's row.
    let header = section_row(ui, "Warp", WARP_MODE_WIDTH + CELL_GAP, select::HEIGHT);
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
    );
    if warp != shown.warp {
        let mut edited = shown.clone();
        edited.set_warp(warp, shown.bpm, shown.transpose, shown.detune, bpm);
        *live = Some((edited, true));
    }
    let warped = shown.warp != WarpMode::Off;
    let stretched = shown.warp == WarpMode::Spectral;
    let mut stretch = shown.stretch;
    let file_bpm = ui.columns(2, |cells| {
        cells[1].add_enabled_ui(stretched, |ui| window_ui(ui, shown, &mut stretch));
        cells[0]
            .add_enabled_ui(warped, |ui| tempo_ui(ui, shown))
            .inner
    });
    if let Some(file_bpm) = file_bpm.filter(|b| BPM_RANGE.contains(b)) {
        let mut edited = shown.clone();
        edited.set_warp(shown.warp, file_bpm, shown.transpose, shown.detune, bpm);
        *change = Some((edited, true));
    }

    ui.add_space(CELL_GAP);
    ui.columns(3, |cells| {
        let mut transpose = shown.transpose;
        let limit = MAX_TRANSPOSE as i32;
        let response = pitch_cell(
            &mut cells[0],
            "Transpose",
            DragValue::new(&mut transpose)
                .speed(0.1)
                .range(-limit..=limit)
                .custom_formatter(|v, _| format!("{v:+.0} st"))
                .custom_parser(|text| text.trim().trim_end_matches("st").trim().parse().ok()),
            shown.transpose as f32 / MAX_TRANSPOSE,
            color,
        );
        if let Some(done) = gesture(&response) {
            let mut edited = shown.clone();
            edited.set_warp(shown.warp, shown.bpm, transpose, shown.detune, bpm);
            *live = Some((edited, done));
        }

        let mut detune = shown.detune;
        let response = pitch_cell(
            &mut cells[1],
            "Detune",
            DragValue::new(&mut detune)
                .speed(0.5)
                .range(-MAX_DETUNE..=MAX_DETUNE)
                .max_decimals(0)
                .custom_formatter(|v, _| format!("{v:+.0} ct"))
                .custom_parser(|text| text.trim().trim_end_matches("ct").trim().parse().ok()),
            shown.detune / MAX_DETUNE,
            color,
        );
        if let Some(done) = gesture(&response) {
            let mut edited = shown.clone();
            edited.set_warp(shown.warp, shown.bpm, shown.transpose, detune, bpm);
            *live = Some((edited, done));
        }

        cells[2].add_enabled_ui(stretched, |ui| formants_ui(ui, &mut stretch));
    });
    if stretch != shown.stretch {
        let mut edited = shown.clone();
        edited.stretch = stretch;
        *live = Some((edited, true));
    }
}

/// A pitch's cell: its field filled in `color` from the centre, to
/// `level` (-1 to 1) of its width either way, as a pan.
fn pitch_cell(
    ui: &mut Ui,
    label: &str,
    value: DragValue,
    level: f32,
    color: Color32,
) -> egui::Response {
    cell_label(ui, label);
    // Painted under the field, whose own background is see-through.
    let under = ui.painter().add(Shape::Noop);
    let width = ui.available_width();
    let response = ui
        .scope(|ui| {
            let widgets = &mut ui.visuals_mut().widgets;
            for w in [
                &mut widgets.inactive,
                &mut widgets.hovered,
                &mut widgets.active,
            ] {
                w.weak_bg_fill = Color32::TRANSPARENT;
            }
            ui.add_sized(vec2(width, FIELD_HEIGHT), value)
        })
        .inner;
    let rect = response.rect;
    let visuals = ui.style().interact(&response);
    let centre = rect.center().x;
    let x = rect
        .lerp_inside(vec2((level.clamp(-1., 1.) + 1.) / 2., 0.))
        .x;
    let fill = Rect::from_x_y_ranges(centre.min(x)..=centre.max(x), rect.y_range());
    let fill_color = visuals.weak_bg_fill.lerp_to_gamma(color, FIELD_LEVEL);
    let radius = visuals.corner_radius;
    ui.painter().set(
        under,
        Shape::Vec(vec![
            Shape::rect_filled(rect, radius, visuals.weak_bg_fill),
            Shape::rect_filled(fill, radius, fill_color),
        ]),
    );
    response
}

/// The stretch's window, picked into `stretch`.
fn window_ui(ui: &mut Ui, shown: &AudioClip, stretch: &mut StretchSettings) {
    cell_label(ui, "Window");
    let width = ui.available_width();
    let mut select = Select::new(("clip-window", shown.id), &mut stretch.window).width(width);
    for window in StretchWindow::ALL {
        select = select.option(window, window_name(window));
    }
    ui.add(select);
}

/// The stretch's formants, kept or moved with the pitch, into `stretch`.
fn formants_ui(ui: &mut Ui, stretch: &mut StretchSettings) {
    cell_label(ui, "Formants");
    let size = vec2(ui.available_width(), FIELD_HEIGHT);
    let on = stretch.formants;
    let toggle = FlatButton::new(if on { "Keep" } else { "Move" })
        .size(size)
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
        egui::TextEdit::singleline(&mut text)
            .horizontal_align(egui::Align::Center)
            .margin(vec2(2., 1.)),
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

/// `120`, `97.5`, `97.25`: up to two decimals.
fn format_number(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::format_number;

    #[test]
    fn tempos_show_the_decimals_they_have() {
        assert_eq!(format_number(120.), "120");
        assert_eq!(format_number(97.5), "97.5");
        assert_eq!(format_number(97.25), "97.25");
        assert_eq!(format_number(100.004), "100");
    }
}
