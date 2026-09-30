//! Effect editors: the UI of each [`EffectKind`], in a frame with a header
//! and power button.

use crate::{
    core::{
        effect::{Effect, EffectKind},
        metrics::AudioMetrics,
    },
    ui::{effects::filter::FilterEditor, font::PHOSPHOR_REGULAR, theme::ThemeExt},
};
use egui::{Button, Frame, Label, Margin, Rect, Response, RichText, Sense, Stroke, Ui, Vec2};
use std::collections::HashMap;
use tonique_engine::edit::{Plugin, PluginId};

pub mod filter;

/// The controls of one kind of effect. The plugin's parameters are the
/// source of truth: read them every frame, set them when edited.
pub trait EffectEditor {
    fn ui(&mut self, ui: &mut Ui, plugin: &Plugin, metrics: &mut AudioMetrics, enabled: bool);
    /// Width of the editor's frame.
    fn width(&self) -> f32;
}

fn editor(kind: EffectKind, plugin: PluginId) -> Box<dyn EffectEditor> {
    match kind {
        EffectKind::Filter => Box::new(FilterEditor::new(plugin)),
    }
}

/// What the user did to an effect's frame.
pub struct EffectResponse {
    /// The header: click to select, drag to move.
    pub header: Response,
    /// Power button clicked.
    pub toggled: bool,
    /// The whole frame.
    pub rect: Rect,
}

/// The editors of the effects shown, made when first shown.
#[derive(Default)]
pub struct EffectRack {
    editors: HashMap<PluginId, Box<dyn EffectEditor>>,
}

impl EffectRack {
    /// Editors made so far.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.editors.len()
    }

    pub fn effect_ui(
        &mut self,
        ui: &mut Ui,
        effect: &Effect,
        metrics: &mut AudioMetrics,
        selected: bool,
    ) -> EffectResponse {
        let editor = self
            .editors
            .entry(effect.plugin.id)
            .or_insert_with(|| editor(effect.kind, effect.plugin.id));
        ui.set_height(ui.available_height());
        let theme = ui.app_theme();
        let stroke_color = if selected { theme.accent } else { theme.border };

        let frame = Frame::new()
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
                            ui.set_width(editor.width());
                            let response = header(ui, effect);
                            editor.ui(ui, &effect.plugin, metrics, effect.enabled());
                            response
                        })
                        .inner
                    })
                    .inner
            });
        EffectResponse {
            rect: frame.response.rect,
            ..frame.inner
        }
    }
}

fn header(ui: &mut Ui, effect: &Effect) -> EffectResponse {
    let theme = ui.app_theme();
    let enabled = effect.enabled();
    let header = ui.interact(
        Rect::from_min_size(
            ui.next_widget_position(),
            Vec2::new(ui.available_width(), 20.),
        ),
        ui.id().with(("effect-header", effect.plugin.id.0)),
        Sense::click_and_drag(),
    );
    let mut toggled = false;

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
                toggled = ui
                    .add(
                        Button::new(
                            RichText::new(egui_phosphor::regular::POWER)
                                .size(8.)
                                .family(egui::FontFamily::Name(PHOSPHOR_REGULAR.into()))
                                .color(if enabled {
                                    theme.text_on_accent
                                } else {
                                    theme.text_muted
                                }),
                        )
                        .small()
                        .fill(if enabled { theme.accent } else { theme.bg_deep })
                        .stroke(Stroke::NONE)
                        .min_size(Vec2::new(15., 15.)),
                    )
                    .clicked();
                ui.add_space(4.0);
                ui.add(
                    Label::new(
                        RichText::new(effect.kind.name())
                            .size(8.0)
                            .color(theme.text),
                    )
                    .selectable(false),
                );
            });
        });
    EffectResponse {
        header,
        toggled,
        rect: Rect::NOTHING,
    }
}
