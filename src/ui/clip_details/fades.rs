//! The inspector's Fades panel: how long they are, and their shapes.

use egui::{DragValue, Ui, vec2};
use tonique_engine::{edit::Fade, nodes::FadeShape};

use crate::{
    core::clip::{AudioClip, Crossfades},
    ui::{arrangement::clip_fades::shape_name, widget::select::Select},
    utils::format_time,
};

use super::inspector::{CELL_GAP, FIELD_HEIGHT, cell, cell_label, gesture, parse_time, section};

/// Which of a clip's fades a cell is for.
#[derive(Clone, Copy, Hash, Debug, PartialEq)]
enum End {
    In,
    Out,
}

/// The fades in and out: lengths, dragged or typed, and shapes. At an end
/// with a crossfade, what's shown is the crossfade, which the handle on its
/// seam sets: greyed out. Edits go in `change`, as in the inspector.
pub(super) fn fades_panel(
    ui: &mut Ui,
    shown: &AudioClip,
    crossed: Crossfades,
    bpm: f32,
    change: &mut Option<(AudioClip, bool)>,
) {
    section(ui, "Fades", 0.);
    let (fade_in, fade_out) = shown.heard_fades(crossed, bpm);
    ui.columns(2, |cells| {
        let in_crossed = crossed.start.is_some();
        let out_crossed = crossed.end.is_some();
        length_cell(
            &mut cells[0],
            "In",
            fade_in,
            !in_crossed,
            |seconds, done| {
                let mut edited = shown.clone();
                edited.set_fade_in(seconds, crossed, bpm);
                *change = Some((edited, done));
            },
        );
        length_cell(
            &mut cells[1],
            "Out",
            fade_out,
            !out_crossed,
            |seconds, done| {
                let mut edited = shown.clone();
                edited.set_fade_out(seconds, crossed, bpm);
                *change = Some((edited, done));
            },
        );
    });
    ui.add_space(CELL_GAP);
    ui.columns(2, |cells| {
        let (left, right) = cells.split_at_mut(1);
        let ends = [
            (&mut left[0], End::In, fade_in, crossed.start.is_some()),
            (&mut right[0], End::Out, fade_out, crossed.end.is_some()),
        ];
        for (cell, end, fade, crossed) in ends {
            cell.add_enabled_ui(!crossed, |ui| {
                if let Some(shape) = shape_cell(ui, shown, end, fade) {
                    let mut edited = shown.clone();
                    match end {
                        End::In => edited.fades.fade_in.curve.shape = shape,
                        End::Out => edited.fades.fade_out.curve.shape = shape,
                    }
                    *change = Some((edited, true));
                }
            });
        }
    });
}

/// A fade's length, in a cell. `edit` gets a new length and whether it's
/// done.
fn length_cell(ui: &mut Ui, label: &str, fade: Fade, enabled: bool, edit: impl FnOnce(f64, bool)) {
    let mut seconds = fade.length_s;
    ui.add_enabled_ui(enabled, |ui| {
        let response = cell(
            ui,
            label,
            DragValue::new(&mut seconds)
                .speed(0.005)
                .range(0.0..=f64::MAX)
                .custom_formatter(|v, _| format_time(v))
                .custom_parser(parse_time),
        );
        if let Some(done) = gesture(&response) {
            edit(seconds, done);
        }
    });
}

/// A fade's shape, in a cell: the new one, if picked.
fn shape_cell(ui: &mut Ui, shown: &AudioClip, end: End, fade: Fade) -> Option<FadeShape> {
    cell_label(ui, "Shape");
    let mut shape = fade.curve.shape;
    let width = ui.available_width();
    let size = vec2(width, FIELD_HEIGHT);
    let mut select = Select::new(("clip-fade-shape", shown.id, end), &mut shape)
        .width(size.x)
        .height(size.y);
    for option in FadeShape::ALL {
        select = select.option(option, shape_name(option));
    }
    ui.add(select);
    (shape != fade.curve.shape).then_some(shape)
}
