use egui::{
    Color32, FontFamily, FontId, Frame, Label, Pos2, RichText, Sense, Shape, Stroke, Ui, vec2,
};
use egui_phosphor::fill::{PLAY, STOP};

use crate::{
    analysis::AudioInfo,
    core::state::{PlaybackState, ToniqueProjectState},
    ui::{
        font::PHOSPHOR_FILL, theme::PRIMARY_COLOR, waveform::UIWaveform,
        widget::square_button::SquareButton,
    },
};

const PREVIEW_WINDOW_HEIGHT: f32 = 60.;
pub struct UIPreview {
    waveform: UIWaveform,
}

impl UIPreview {
    pub fn new() -> Self {
        Self {
            waveform: UIWaveform::new(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState, selected_audio: &AudioInfo) {
        Frame::new()
            .stroke(Stroke::new(4.0, Color32::from_gray(100)))
            .show(ui, |ui| {
                ui.set_height(PREVIEW_WINDOW_HEIGHT - 2.0 * 4.0);
                ui.horizontal(|ui| {
                    self.waveform_ui(ui, selected_audio, state, ui.available_width() - 17.);
                    self.play_control_ui(ui, state, selected_audio);
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
                    Label::new(
                        RichText::new(format!(
                            "Length: {:.3}s",
                            selected_audio.duration.unwrap().as_secs_f32()
                        ))
                        .size(9.),
                    )
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

    fn waveform_ui(
        &self,
        ui: &mut Ui,
        audio: &AudioInfo,
        state: &mut ToniqueProjectState,
        width: f32,
    ) {
        let (response, painter) = ui.allocate_painter(vec2(width, 14.), Sense::click());
        let rect = response.rect;

        if response.clicked()
            && let Some(mouse_pos) = response.interact_pointer_pos()
        {
            state.seek_preview(
                ((mouse_pos.x - rect.left()) / rect.width() * audio.num_samples.unwrap() as f32)
                    .round() as usize,
            );
        }
        let mut shapes = vec![];
        shapes.push(Shape::line_segment(
            [
                Pos2::new(rect.left(), rect.center().y),
                Pos2::new(rect.right(), rect.center().y),
            ],
            Stroke::new(1.0, Color32::from_black_alpha(80)),
        ));

        if let Ok(data) = audio.data.read() {
            self.waveform.paint(
                &mut shapes,
                response.rect,
                data,
                0.,
                1.,
                audio.num_samples.unwrap(),
                false,
                PRIMARY_COLOR,
            );
        }

        let x = response.rect.left()
            + state.preview_position() as f32 / audio.num_samples.unwrap() as f32
                * response.rect.width();
        shapes.push(Shape::line_segment(
            [
                Pos2::new(x, response.rect.top()),
                Pos2::new(x, response.rect.bottom()),
            ],
            Stroke::new(1.0, Color32::from_white_alpha(200)),
        ));
        painter.add(shapes);
    }

    fn play_control_ui(&self, ui: &mut Ui, state: &mut ToniqueProjectState, audio: &AudioInfo) {
        if ui
            .add(
                SquareButton::ghost(if state.preview_playback_state() == PlaybackState::Paused {
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
}
