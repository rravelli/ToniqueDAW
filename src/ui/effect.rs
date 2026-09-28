use crate::{
    core::metrics::AudioMetrics,
    ui::{effects::EffectId, theme::ThemeExt},
};
use egui::{
    Button, Frame, InnerResponse, Label, Margin, Rect, Response, RichText, Sense, Stroke, Ui, Vec2,
};
use std::fmt::Debug;
use tonique_engine::edit::{Plugin, PluginId, PluginKind, TrackId};

pub trait EffectEditor: EffectEditorClone {
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
    // engine plugin that does the processing
    fn plugin_kind(&self) -> PluginKind;
    // take the plugin's parameters, so the editor controls them
    fn bind(&mut self, plugin: &Plugin);
    // effect id
    fn id(&self) -> String;
    /// Which effect this is, as saved in projects.
    fn effect_id(&self) -> EffectId;
    /// Update the editor from its bound parameters (after a project load).
    fn read_params(&mut self);
}

pub trait EffectEditorClone {
    fn clone_box(&self) -> Box<dyn EffectEditor>;
}

#[derive(Clone)]
pub struct EffectSlot {
    id: String,
    pub track_id: TrackId,
    plugin_id: PluginId,
    pub enabled: bool,
    /// Power button clicked since the last `take_toggled`.
    toggled: bool,
    pub name: String,
    content: Box<dyn EffectEditor>,
}

impl<T> EffectEditorClone for T
where
    T: 'static + EffectEditor + Clone,
{
    fn clone_box(&self) -> Box<dyn EffectEditor> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn EffectEditor> {
    fn clone(&self) -> Box<dyn EffectEditor> {
        self.clone_box()
    }
}

impl Debug for EffectSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EffectSlot")
            .field("id", &self.id)
            .field("track_id", &self.track_id)
            .field("plugin_id", &self.plugin_id)
            .field("enabled", &self.enabled)
            .field("name", &self.name)
            .field("content", &self.content.id())
            .finish()
    }
}

impl EffectSlot {
    pub fn new(mut content: Box<dyn EffectEditor>, track_id: TrackId, plugin: &Plugin) -> Self {
        content.bind(plugin);
        Self {
            id: content.id(),
            plugin_id: plugin.id,
            enabled: !plugin.bypassed,
            toggled: false,
            name: "Audio effect".to_string(),
            content,
            track_id,
        }
    }

    /// Point this editor at another plugin (e.g. the copy on a duplicated track).
    pub fn bind(&mut self, track_id: TrackId, plugin: &Plugin) {
        self.content.bind(plugin);
        self.track_id = track_id;
        self.plugin_id = plugin.id;
        self.id = format!("{}-{}", self.content.id(), plugin.id.0);
    }

    pub fn plugin_id(&self) -> PluginId {
        self.plugin_id
    }

    /// Power button: the state turns this into a bypass change.
    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
        self.toggled = true;
    }

    pub fn effect_id(&self) -> EffectId {
        self.content.effect_id()
    }

    /// See [`EffectEditor::read_params`].
    pub fn read_params(&mut self) {
        self.content.read_params();
    }

    pub fn take_toggled(&mut self) -> bool {
        std::mem::take(&mut self.toggled)
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        metrics: &mut AudioMetrics,
        selected: bool,
        // state: &mut ToniqueProjectState,
    ) -> InnerResponse<Response> {
        ui.set_height(ui.available_height());
        let theme = ui.app_theme();
        let stroke_color = if selected { theme.accent } else { theme.border };

        let response = Frame::new()
            .fill(theme.bg_raised)
            .stroke(Stroke::new(1.0 / ui.pixels_per_point(), stroke_color))
            .corner_radius(2.0)
            .show(ui, |ui| {
                Frame::new()
                    .corner_radius(2.0)
                    .stroke(Stroke::new(2.0, theme.border))
                    .show(ui, |ui| {
                        ui.set_height(ui.available_height());
                        ui.vertical(|ui| {
                            ui.set_width(self.content.width());
                            let bar_response = self.header(ui);
                            self.content.ui(ui, metrics, self.enabled);
                            bar_response
                        })
                        .inner
                    })
                    .inner
            });

        response
    }

    fn header(&mut self, ui: &mut Ui) -> Response {
        let theme = ui.app_theme();
        let response = ui.interact(
            Rect::from_min_size(
                ui.next_widget_position(),
                Vec2::new(ui.available_width(), 20.),
            ),
            self.id.clone().into(),
            Sense::click_and_drag(),
        );

        Frame::new()
            .fill(theme.bg_control)
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
                                    .color(if self.enabled {
                                        theme.text_on_accent
                                    } else {
                                        theme.text_muted
                                    }),
                            )
                            .small()
                            .fill(if self.enabled {
                                theme.accent
                            } else {
                                theme.bg_deep
                            })
                            .stroke(Stroke::NONE)
                            .min_size(Vec2::new(15., 15.)),
                        )
                        .clicked()
                    {
                        self.toggle();
                    };
                    ui.add_space(4.0);
                    ui.add(
                        Label::new(RichText::new(self.name.clone()).size(8.0).color(theme.text))
                            .selectable(false),
                    );
                });
            });
        response
    }
}
