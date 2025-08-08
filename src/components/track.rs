use egui::{
    Align2, Color32, FontId, Frame, Label, Pos2, Rect, Response, RichText, Sense, Stroke, TextEdit,
    Ui, Vec2,
};
use rand::Rng;
use rtrb::Producer;
use std::ops::RangeInclusive;

use crate::{
    components::{
        clip::UIClip,
        effect::UIEffect,
        effects::{EffectId, create_effect_from_id},
        loudness_meter::LoudnessMeter,
    },
    message::GuiToPlayerMsg,
    metrics::AudioMetrics,
};

pub const DEFAULT_TRACK_HEIGHT: f32 = 60.;
const STROKE_WIDTH: f32 = 0.5;
const PADDING: f32 = 4.;
const CLOSED_HEIGHT: f32 = 2. * (STROKE_WIDTH + PADDING) + 20.;
const METER_WIDTH: f32 = 8.;
pub const HANDLE_HEIGHT: f32 = 3.0;

pub enum TrackSoloState {
    Soloing,
    NotSoloing,
    Solo,
}

#[derive(Clone)]
pub struct UITrack {
    pub id: String,
    pub name: String,
    pub height: f32,
    pub clips: Vec<UIClip>,
    pub muted: bool,
    pub volume: f32,
    pub closed: bool,
    pub color: Color32,
    effects: Vec<UIEffect>,
    // volume in DB
    gain: f32,
    prev_height: f32,
    loudness_meter: LoudnessMeter,
    edit: bool,
}

impl UITrack {
    pub fn new(id: &str, name: &str) -> Self {
        let mut rng = rand::rng();
        let color = Color32::from_rgb(
            rng.random_range(0..=255),
            rng.random_range(0..=255),
            rng.random_range(0..=255),
        );
        Self {
            id: id.to_string(),
            name: name.to_string(),
            height: DEFAULT_TRACK_HEIGHT,
            prev_height: DEFAULT_TRACK_HEIGHT,
            clips: vec![],
            muted: false,
            volume: 0.,
            gain: 0.,
            closed: false,
            loudness_meter: LoudnessMeter {},
            color,
            edit: false,
            effects: vec![],
        }
    }

    // Add sample and fixes collisions
    pub fn add_clip(&mut self, clip: UIClip, bpm: f32, tx: &mut Producer<GuiToPlayerMsg>) {
        let mut new_samples = vec![];
        let start = clip.position;
        let end = clip.end(bpm);

        // Fix overlap
        for clip in self.clips.iter_mut() {
            // No overlap
            if clip.position > end || clip.end(bpm) < start {
                new_samples.push(clip.clone());
                continue;
            }
            let _ = tx.push(GuiToPlayerMsg::RemoveClip(vec![clip.id()]));
            // Sample overlaps before new clip
            if clip.position < start {
                let mut trimmed = clip.clone_with_new_id();
                trimmed.trim_end_at(start, bpm);
                let _ = tx.push(GuiToPlayerMsg::AddClip(
                    self.id.clone(),
                    trimmed.audio.path.clone(),
                    trimmed.position,
                    trimmed.id().clone(),
                    trimmed.trim_start,
                    trimmed.trim_end,
                ));
                new_samples.push(trimmed);
            }

            // // Sample overlaps after new clip
            if clip.end(bpm) > end {
                let mut trimmed = clip.clone_with_new_id();

                trimmed.trim_start_at(end, bpm);
                let _ = tx.push(GuiToPlayerMsg::AddClip(
                    self.id.clone(),
                    trimmed.audio.path.clone(),
                    trimmed.position,
                    trimmed.id().clone(),
                    trimmed.trim_start,
                    trimmed.trim_end,
                ));
                new_samples.push(trimmed);
            }
        }

        new_samples.push(clip);
        self.clips = new_samples;
    }

    pub fn delete_ids(&mut self, ids: Vec<String>) {
        let mut new_clips = vec![];
        for clip in self.clips.iter_mut() {
            if !ids.contains(&clip.id()) {
                new_clips.push(clip.clone());
            }
        }

        self.clips = new_clips;
    }

    pub fn add_effect(&mut self, id: EffectId, index: usize, tx: &mut Producer<GuiToPlayerMsg>) {
        let effect = create_effect_from_id(id);
        let _ = tx.push(GuiToPlayerMsg::AddNode(
            self.id.clone(),
            index,
            effect.id(),
            effect.get_unit(),
        ));

        self.effects
            .insert(index, UIEffect::new(effect, self.id.clone()));
    }

    pub fn remove_effects(&mut self, indexes: &Vec<usize>, tx: &mut Producer<GuiToPlayerMsg>) {
        let mut new_effects = Vec::new();
        for (i, effect) in self.effects.iter().enumerate() {
            if !indexes.contains(&i) {
                new_effects.push(effect.clone());
            } else {
                let _ = tx.push(GuiToPlayerMsg::RemoveNode(self.id.clone(), effect.id()));
            }
        }
        self.effects = new_effects;
    }

    pub fn effects_mut(&mut self) -> &mut [UIEffect] {
        &mut self.effects
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        metrics: AudioMetrics,
        solo: TrackSoloState,
        selected: bool,
    ) -> (bool, bool, bool, bool, bool, Response) {
        let mut mute_changed = false;
        let mut volume_changed = false;
        let mut solo_clicked = false;
        let mut clicked = false;
        let mut double_clicked = false;
        let res = Frame::new()
            .fill(Color32::from_gray(30)) // Background color (optional)
            .stroke(Stroke::new(STROKE_WIDTH, Color32::from_gray(50))) // Border thickness and color
            .show(ui, |ui| {
                ui.style_mut().spacing.item_spacing = Vec2::new(2.0, 2.0);
                ui.set_height(self.height - 2. * STROKE_WIDTH);
                ui.set_width(ui.available_width());
                let horizontal_response = ui.horizontal_top(|ui| {
                    let response = ui.interact(
                        Rect::from_min_size(
                            ui.next_widget_position(),
                            Vec2::new(
                                ui.available_width() - 50.,
                                self.height - 2. * PADDING - 2. * STROKE_WIDTH,
                            ),
                        ),
                        ui.make_persistent_id(format!("track-{}", self.id)),
                        Sense::click_and_drag(),
                    );
                    let mut frame_color = self.color;
                    if selected {
                        frame_color = frame_color.blend(Color32::from_black_alpha(40));
                    }
                    self.header_ui(ui, frame_color);
                    response.context_menu(|ui| {
                        self.context_menu(ui);
                    });
                    clicked = response.clicked();
                    double_clicked = response.double_clicked();
                    (mute_changed, solo_clicked, volume_changed) = self.control_ui(ui, &solo);
                    response
                });
                horizontal_response.inner
            });

        let loudness_rect = Rect::from_min_size(
            Pos2::new(
                res.response.rect.max.x - METER_WIDTH - STROKE_WIDTH - 4.0,
                res.response.rect.min.y + STROKE_WIDTH,
            ),
            Vec2::new(METER_WIDTH, self.height - 2. * STROKE_WIDTH),
        );

        self.loudness_meter.paint(
            &ui.painter(),
            loudness_rect,
            metrics,
            self.muted && !matches!(solo, TrackSoloState::Solo)
                || matches!(solo, TrackSoloState::Soloing),
        );
        self.dragger(ui);
        (
            mute_changed,
            volume_changed,
            solo_clicked,
            clicked,
            double_clicked,
            res.inner,
        )
    }

    fn context_menu(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            if ui.button("Rename").clicked() {
                self.edit = true;
            };
            let _ = ui.button("Delete");
            let _ = ui.button("Duplicate");
        });
    }

    fn header_ui(&mut self, ui: &mut Ui, color: Color32) {
        Frame::new()
            .fill(color)
            .inner_margin(Vec2::new(PADDING, PADDING))
            .show(ui, |ui| {
                ui.set_width(ui.available_width() - 50.);
                ui.set_height(self.height - 2. * PADDING - 2. * STROKE_WIDTH);
                ui.horizontal(|ui| {
                    self.open_button(ui);
                    if self.edit {
                        let text_edit = ui.add(
                            TextEdit::singleline(&mut self.name)
                                .font(FontId::new(9., egui::FontFamily::Proportional))
                                .background_color(Color32::from_black_alpha(20))
                                .text_color(Color32::from_gray(20))
                                .desired_width(ui.available_width() - 40.),
                        );
                        if text_edit.lost_focus() {
                            self.edit = false;
                            if self.name.is_empty() {
                                self.name = "Audio Track".to_string();
                            }
                        }
                    } else {
                        ui.add(
                            Label::new(
                                RichText::new(self.name.clone())
                                    .color(Color32::from_gray(20))
                                    .size(9.),
                            )
                            .truncate()
                            .selectable(false),
                        );
                    }

                    if ui
                        .small_button(RichText::new(egui_phosphor::regular::PAINT_BRUSH).size(8.))
                        .clicked()
                    {
                        let mut rng = rand::rng();
                        self.color = Color32::from_rgb(
                            rng.random_range(0..=255),
                            rng.random_range(0..=255),
                            rng.random_range(0..=255),
                        )
                    }
                });
            });
    }

    fn control_ui(&mut self, ui: &mut Ui, solo: &TrackSoloState) -> (bool, bool, bool) {
        let mut mute_changed = false;
        let mut solo_clicked = false;
        let mut volume_changed = false;
        Frame::new()
            .inner_margin(Vec2::new(0., PADDING))
            .show(ui, |ui| {
                ui.set_width(50. - 2. * PADDING);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        let mute_btn = self.mute_button(ui);
                        let solo_res = self.solo_button(ui, matches!(solo, TrackSoloState::Solo));

                        if mute_btn.clicked() {
                            self.muted = !self.muted;
                            mute_changed = true;
                        };

                        if solo_res.clicked() {
                            solo_clicked = true;
                        }
                    });
                    if !self.closed {
                        let prev_gain = self.gain;
                        self.gain_slider(ui, RangeInclusive::new(-40., 5.));
                        volume_changed = prev_gain != self.gain;
                    }
                });
            });

        (mute_changed, solo_clicked, volume_changed)
    }

    fn mute_button(&mut self, ui: &mut Ui) -> Response {
        ui.add(
            egui::Button::new(RichText::new("M").size(8.)).fill(if self.muted {
                ui.visuals().selection.bg_fill
            } else {
                ui.visuals().widgets.inactive.bg_fill
            }),
        )
    }

    fn solo_button(&mut self, ui: &mut Ui, solo: bool) -> Response {
        ui.add(
            egui::Button::new(RichText::new("S").size(8.)).fill(if solo {
                ui.visuals().selection.bg_fill
            } else {
                ui.visuals().widgets.inactive.bg_fill
            }),
        )
    }

    fn open_button(&mut self, ui: &mut Ui) -> Response {
        let icon = if self.closed {
            egui_phosphor::fill::CARET_RIGHT
        } else {
            egui_phosphor::fill::CARET_DOWN
        };
        let response = ui.add(
            egui::Button::new(RichText::new(icon).color(Color32::from_gray(20)).size(7.))
                .small()
                .min_size(Vec2::new(14., 14.))
                .fill(Color32::TRANSPARENT)
                .corner_radius(10.)
                .stroke(Stroke::new(1., Color32::from_gray(20).gamma_multiply(0.5))),
        );

        if response.clicked() {
            self.closed = !self.closed;
            if self.closed {
                self.height = CLOSED_HEIGHT;
            } else {
                self.height = self.prev_height;
            }
        }

        response
    }

    fn gain_slider(&mut self, ui: &mut Ui, range: std::ops::RangeInclusive<f32>) -> Response {
        let desired_size = egui::vec2(32., 20.);
        let (rect, mut response) = ui.allocate_exact_size(desired_size, Sense::click_and_drag());

        if response.dragged() {
            let delta = response.drag_delta().x;
            self.gain += delta * (range.end() - range.start()) / rect.width();
            self.gain = self.gain.clamp(*range.start(), *range.end());
            response.mark_changed();
        }

        if response.double_clicked() {
            self.gain = 0.;
        }

        if response.hovered() {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
        }

        // Compute fill ratio
        let t = (self.gain - *range.start()) / (*range.end() - *range.start());

        // Paint background bar
        let visuals = ui.style().visuals.clone();
        let bg_fill = visuals.extreme_bg_color;
        let fill_color = visuals.selection.bg_fill;

        let painter = ui.painter();

        // Full background
        painter.rect_filled(rect, 1.0, bg_fill);

        // Filled ratio bar
        let fill_rect = Rect::from_min_max(
            rect.min,
            Pos2::new(rect.left() + rect.width() * t, rect.bottom()),
        );
        painter.rect_filled(fill_rect, 2.0, fill_color);

        // Text value
        let text = format!("{:.1}", self.gain);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::new(10., egui::FontFamily::Proportional),
            visuals.text_color(),
        );

        self.volume = 10f32.powf(self.gain / 20.);

        response
    }

    fn dragger(&mut self, ui: &mut Ui) {
        let (_, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), HANDLE_HEIGHT),
            Sense::drag(),
        );

        if response.hovered() {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeVertical);
        }
        if response.dragged() && !self.closed {
            self.height += response.drag_delta().y;
            self.height = self.height.clamp(CLOSED_HEIGHT, 400.);
            self.prev_height = self.height;
        }
    }
}
