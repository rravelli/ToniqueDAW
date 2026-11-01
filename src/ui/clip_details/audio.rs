//! The inspector's Audio panel: how the clip sounds (gain, pan, its
//! transposition), and how it follows the project's tempo (its file's
//! tempo, how it's stretched).

use egui::{Color32, DragValue, FontId, Rect, Shape, Ui, vec2};
use tonique_engine::edit::{MAX_TRANSPOSE, StretchSettings, StretchWindow, WarpMode};

use crate::{
    core::{
        clip::{AudioClip, MAX_DETUNE},
        state::{ClipParam, ProjectState},
    },
    ui::{
        theme::font_size,
        widget::{
            flat_button::FlatButton,
            param_slider::{Drag, DragStep, FIELD_LEVEL, ParamSlider, anchored_drag},
            select::Select,
        },
    },
};

use super::inspector::{
    CELL_GAP, CLIP_GAIN_RANGE, FIELD_HEIGHT, apply_param, cell_label, section, switch_section,
};

/// The file's tempo halved or doubled, beside it.
const TEMPO_BUTTON_WIDTH: f32 = 26.;
/// The file's tempo as typed: `120.00` and room around it.
const TEMPO_FIELD_WIDTH: f32 = 72.;
/// A file's tempo, as typed.
const BPM_RANGE: std::ops::RangeInclusive<f64> = 10.0..=999.0;

/// Gain and pan, sliders like a track's heard as they're dragged; the
/// transposition, heard as it's dragged (`live`) and filled in the clip's
/// `color`; the warp, switched; the file's tempo, typed or halved and
/// doubled; the stretch's window and formants. Edits go in `change` (shown
/// until done) or `live`, as in the inspector.
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

    // Heard as they're dragged, one undo step once released.
    ui.add_space(CELL_GAP);
    ui.columns(2, |cells| {
        let id = ("clip-transpose", shown.id);
        let transpose = shown.transpose as f32;
        let edit = pitch_cell(
            &mut cells[0],
            id,
            "Transpose",
            transpose,
            MAX_TRANSPOSE,
            "st",
            color,
        );
        if let Some((transpose, done)) = edit {
            let mut edited = shown.clone();
            edited.set_warp(shown.warp, shown.bpm, transpose as i32, shown.detune, bpm);
            *live = Some((edited, done));
        }

        let id = ("clip-detune", shown.id);
        let edit = pitch_cell(
            &mut cells[1],
            id,
            "Detune",
            shown.detune,
            MAX_DETUNE,
            "ct",
            color,
        );
        if let Some((detune, done)) = edit {
            let mut edited = shown.clone();
            edited.set_warp(shown.warp, shown.bpm, shown.transpose, detune, bpm);
            *live = Some((edited, done));
        }
    });

    // Off, it shows how it would play, greyed out, as the loop.
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
    ui.add_enabled_ui(warped, |ui| {
        if let Some(file_bpm) = tempo_ui(ui, shown).filter(|b| BPM_RANGE.contains(b)) {
            let mut edited = shown.clone();
            edited.set_warp(shown.warp, file_bpm, shown.transpose, shown.detune, bpm);
            *change = Some((edited, true));
        }

        let mut stretch = shown.stretch;
        ui.add_space(CELL_GAP);
        ui.columns(2, |cells| {
            window_ui(&mut cells[0], shown, &mut stretch);
            formants_ui(&mut cells[1], &mut stretch);
        });
        if stretch != shown.stretch {
            let mut edited = shown.clone();
            edited.stretch = stretch;
            *live = Some((edited, true));
        }
    });
}

/// A pitch's cell: a field of whole steps from `-limit` to `limit`
/// (printed with `unit`), filled in `color` from the centre, as a pan.
/// Dragged, the fill follows the pointer (Shift for fine steps); clicked,
/// it's typed. Returns the new value and whether the edit is done.
fn pitch_cell(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    value: f32,
    limit: f32,
    unit: &'static str,
    color: Color32,
) -> Option<(f32, bool)> {
    cell_label(ui, label);
    // Painted under the field, whose own background is see-through.
    let under = ui.painter().add(Shape::Noop);
    let width = ui.available_width();
    let mut typed = value;
    // Its own drag does nothing: the drag below follows the pointer.
    let field = DragValue::new(&mut typed)
        .speed(0.)
        .range(-limit..=limit)
        .max_decimals(0)
        .custom_formatter(move |v, _| format!("{v:+.0} {unit}"))
        .custom_parser(move |text| text.trim().trim_end_matches(unit).trim().parse().ok());
    let response = ui
        .push_id(id, |ui| {
            let widgets = &mut ui.visuals_mut().widgets;
            for w in [
                &mut widgets.inactive,
                &mut widgets.hovered,
                &mut widgets.active,
            ] {
                w.weak_bg_fill = Color32::TRANSPARENT;
            }
            ui.add_sized(vec2(width, FIELD_HEIGHT), field)
        })
        .inner;
    let rect = response.rect;
    let visuals = ui.style().interact(&response);
    let centre = rect.center().x;
    let x = rect
        .lerp_inside(vec2((value / limit).clamp(-1., 1.) / 2. + 0.5, 0.))
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

    let drag_id = response.id.with("drag");
    let mut drag = ui.data(|d| d.get_temp(drag_id));
    let step = anchored_drag(ui, &response, &mut drag, value, value, &(-limit..=limit));
    ui.data_mut(|d| match drag {
        Some(drag) => {
            d.insert_temp(drag_id, drag);
        }
        None => d.remove::<Drag>(drag_id),
    });
    match step {
        DragStep::Moved(dragged) => Some((dragged.round(), false)).filter(|(v, _)| *v != value),
        DragStep::Released { .. } => Some((value, true)),
        DragStep::None => {
            (response.changed() && !response.dragged()).then_some((typed.round(), true))
        }
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
