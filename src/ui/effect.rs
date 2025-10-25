use crate::core::metrics::AudioMetrics;
use egui::{
    Button, Color32, Frame, InnerResponse, Label, Margin, Rect, Response, RichText, Sense, Stroke,
    Ui, Vec2,
};
use fundsp::hacker::AudioUnit;
use std::fmt::Debug;

pub trait UIEffectContent: UIEffectContentClone {
    // show ui and update effect
    fn ui(
        &mut self,
        ui: &mut Ui,
        metrics: &mut AudioMetrics,
        enabled: bool,
        // tx: &mut Producer<GuiToPlayerMsg>,
    );
    // effect window width
    fn width(&self) -> f32;
    // get audio processing unit
    fn get_unit(&self) -> Box<dyn AudioUnit>;
    // effect id
    fn id(&self) -> String;
}

pub trait UIEffectContentClone {
    fn clone_box(&self) -> Box<dyn UIEffectContent>;
}

#[derive(Clone)]
pub struct UIEffect {
    id: String,
    pub track_id: String,
    pub enabled: bool,
    pub name: String,
    content: Box<dyn UIEffectContent>,
}

impl<T> UIEffectContentClone for T
where
    T: 'static + UIEffectContent + Clone,
{
    fn clone_box(&self) -> Box<dyn UIEffectContent> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn UIEffectContent> {
    fn clone(&self) -> Box<dyn UIEffectContent> {
        self.clone_box()
    }
}

impl Debug for UIEffect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UIEffect")
            .field("id", &self.id)
            .field("track_id", &self.track_id)
            .field("enabled", &self.enabled)
            .field("name", &self.name)
            .field("content", &self.content.id())
            .finish()
    }
}

impl UIEffect {
    pub fn new(content: Box<dyn UIEffectContent>, track_id: String) -> Self {
        Self {
            id: content.id(),
            enabled: true,
            name: "Audio effect".to_string(),
            content,
            track_id,
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        metrics: &mut AudioMetrics,
        selected: bool,
        // state: &mut ToniqueProjectState,
    ) -> InnerResponse<Response> {
        ui.set_height(ui.available_height());
        let stroke_color = if selected {
            Color32::WHITE
        } else {
            Color32::from_gray(100)
        };

        let response = Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .stroke(Stroke::new(1.0 / ui.pixels_per_point(), stroke_color))
            .corner_radius(2.0)
            .show(ui, |ui| {
                Frame::new()
                    .corner_radius(2.0)
                    .stroke(Stroke::new(2.0, Color32::from_gray(100)))
                    .show(ui, |ui| {
                        ui.set_height(ui.available_height());
                        ui.vertical(|ui| {
                            ui.set_width(self.content.width());
                            let bar_response = self.top_bar(ui);
                            self.content.ui(ui, metrics, self.enabled);
                            bar_response
                        })
                        .inner
                    })
                    .inner
            });

        response
    }

    fn top_bar(&mut self, ui: &mut Ui) -> Response {
        let response = ui.interact(
            Rect::from_min_size(
                ui.next_widget_position(),
                Vec2::new(ui.available_width(), 20.),
            ),
            self.id.clone().into(),
            Sense::click_and_drag(),
        );

        Frame::new()
            .fill(Color32::from_gray(100))
            .inner_margin(Margin {
                bottom: 0,
                top: 0,
                left: 4,
                right: 4,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            Button::new(
                                RichText::new(egui_phosphor::regular::POWER)
                                    .size(8.)
                                    .family(egui::FontFamily::Name("phosphor_regular".into()))
                                    .color(Color32::from_gray(20)),
                            )
                            .small()
                            .fill(if self.enabled {
                                ui.visuals().selection.bg_fill
                            } else {
                                Color32::from_gray(60)
                            })
                            .stroke(Stroke::new(0.5, Color32::from_gray(20)))
                            .min_size(Vec2::new(15., 15.)),
                        )
                        .clicked()
                    {
                        self.enabled = !self.enabled;
                        // let _ = tx.push(GuiToPlayerMsg::SetNodeEnabled(
                        //     self.track_id.clone(),
                        //     self.content.id(),
                        //     self.enabled,
                        // ));
                    };
                    ui.add_space(4.0);
                    ui.add(
                        Label::new(
                            RichText::new(self.name.clone())
                                .size(8.0)
                                .color(Color32::from_gray(20)),
                        )
                        .selectable(false),
                    );
                });
            });
        response
    }

    pub fn id(&self) -> String {
        self.id.clone()
    }
}
