//! The selected file's preview, under the browser: waveform, play button
//! and details.

use egui::{FontFamily, FontId, Frame, Label, Pos2, RichText, Sense, Shape, Stroke, Ui, vec2};
use egui_phosphor::fill::{PLAY, STOP};

use crate::{
    analysis::AudioInfo,
    core::state::{PlaybackState, ProjectState},
    ui::{
        font::PHOSPHOR_FILL, theme::ThemeExt, waveform::paint_waveform,
        widget::flat_button::FlatButton,
    },
};

/// Height of the preview.
pub const PREVIEW_HEIGHT: f32 = 60.;

pub fn preview_ui(ui: &mut Ui, state: &mut ProjectState, selected_audio: &AudioInfo) {
    // Some files don't say how long they are: known once decoded.
    let length = match selected_audio.total_frames() {
        Some(frames) => format!("{:.3}s", frames / selected_audio.sample_rate as f64),
        None => "unknown".into(),
    };
    Frame::new()
        .stroke(Stroke::new(4.0, ui.app_theme().bg_control))
        .show(ui, |ui| {
            ui.set_height(PREVIEW_HEIGHT - 2.0 * 4.0);
            ui.horizontal(|ui| {
                waveform_ui(ui, selected_audio, state, ui.available_width() - 17.);
                play_control_ui(ui, state, selected_audio);
            });

            ui.add(
                Label::new(
                    RichText::new(selected_audio.name.clone())
                        .strong()
                        .size(12.),
                )
                .selectable(false)
                .wrap_mode(egui::TextWrapMode::Truncate),
            );
            ui.add(
                Label::new(RichText::new(format!("Length: {length}")).size(9.))
                    .selectable(false)
                    .wrap_mode(egui::TextWrapMode::Truncate),
            );
            ui.add(
                Label::new(
                    RichText::new(format!(
                        "Format: {:.1}kHz {}-bit",
                        selected_audio.sample_rate as f32 / 1000.,
                        selected_audio.bit_depth.unwrap_or(16),
                    ))
                    .size(9.),
                )
                .selectable(false)
                .wrap_mode(egui::TextWrapMode::Truncate),
            );
        });
}

fn waveform_ui(ui: &mut Ui, audio: &AudioInfo, state: &mut ProjectState, width: f32) {
    let (response, painter) = ui.allocate_painter(vec2(width, 14.), Sense::click());
    let rect = response.rect;
    let theme = ui.app_theme();

    let frames = audio.total_frames();
    if response.clicked()
        && let Some(mouse_pos) = response.interact_pointer_pos()
        && let Some(frames) = frames
    {
        state.seek_preview(
            ((mouse_pos.x - rect.left()) / rect.width() * frames as f32).round() as usize,
        );
    }
    let mut shapes = vec![];
    shapes.push(Shape::line_segment(
        [
            Pos2::new(rect.left(), rect.center().y),
            Pos2::new(rect.right(), rect.center().y),
        ],
        Stroke::new(1.0, theme.separator),
    ));

    painter.add(std::mem::take(&mut shapes));
    if !audio.data.is_ready() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
    if let Some(frames) = frames {
        paint_waveform(
            &painter,
            rect,
            rect,
            &audio.data,
            0.0..frames,
            false,
            theme.accent,
        );
    }

    if let Some(frames) = frames {
        let x = response.rect.left()
            + state.preview_position() as f32 / frames as f32 * response.rect.width();
        shapes.push(Shape::line_segment(
            [
                Pos2::new(x, response.rect.top()),
                Pos2::new(x, response.rect.bottom()),
            ],
            Stroke::new(1.0, theme.playhead),
        ));
    }
    painter.add(shapes);
}

fn play_control_ui(ui: &mut Ui, state: &mut ProjectState, audio: &AudioInfo) {
    if ui
        .add(
            FlatButton::ghost(if state.preview_playback_state() == PlaybackState::Paused {
                PLAY
            } else {
                STOP
            })
            .square(17.)
            .font(FontId::new(12., FontFamily::Name(PHOSPHOR_FILL.into()))),
        )
        .clicked()
    {
        if state.preview_playback_state() == PlaybackState::Playing {
            state.pause_preview();
        } else {
            state.play_preview(audio.path.clone());
        }
    }
}
