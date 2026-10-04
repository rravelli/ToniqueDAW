//! Effect editors: the UI of each [`EffectKind`], in a frame with a header
//! holding the power and remove buttons.

use crate::{
    core::{
        effect::{Effect, EffectKind, Setting},
        metrics::AudioMetrics,
    },
    ui::{
        effects::{echo::EchoEditor, filter::FilterEditor},
        font::PHOSPHOR_REGULAR,
        theme::ThemeExt,
        widget::{flat_button::FlatButton, knob::Knob},
    },
};
use egui::{
    Align, Align2, FontFamily, FontId, Layout, Rect, Response, Sense, Stroke, Ui, UiBuilder, Vec2,
    vec2,
};
use egui_phosphor::regular::{POWER, X};
use std::{collections::HashMap, ops::RangeInclusive};
use tonique_engine::{
    edit::{Parameter, PluginId},
    param::ParamId,
};

pub mod echo;
pub mod filter;

const HEADER_HEIGHT: f32 = 20.;

/// What an editor changed, for the panel to apply.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EffectEdit {
    /// A parameter, already set live: record it as an undo step from `old`.
    Param { id: ParamId, old: f32, new: f32 },
    /// Swap the plugin for one with this setting.
    Setting(Setting),
}

/// What an editor works with.
pub struct EditorContext<'a> {
    pub effect: &'a Effect,
    /// The engine's, to draw responses as heard.
    pub sample_rate: f32,
    pub metrics: &'a AudioMetrics,
    /// Filled by the editor.
    pub edits: Vec<EffectEdit>,
}

impl EditorContext<'_> {
    pub fn enabled(&self) -> bool {
        self.effect.enabled()
    }

    pub fn param(&self, name: &str) -> Option<&Parameter> {
        self.effect.plugin.param(name)
    }

    /// Knob for a parameter: sets it live while dragged, and records one
    /// undo step on release.
    pub fn param_knob(
        &mut self,
        ui: &mut Ui,
        param: &str,
        name: &str,
        format: &dyn Fn(f32) -> String,
        log: bool,
    ) {
        let Some(p) = self.param(param) else {
            return;
        };
        let (id, before) = (p.id, p.get());
        let mut value = before;
        let response = ui.add(
            Knob::new(&mut value, p.min..=p.max)
                .log(log)
                .default(self.effect.kind.initial_value(&self.effect.plugin, param))
                .name(name)
                .format(format)
                .active(self.enabled()),
        );
        if response.changed() {
            p.set(value);
        }
        if let Some(old) = track_drag(ui, &response, before) {
            self.edits.push(EffectEdit::Param {
                id,
                old,
                new: p.get(),
            });
        }
    }

    /// Knob for a [`Setting`]'s value: shows the value while dragged, and
    /// only changes the setting (which replaces the plugin) on release.
    #[allow(clippy::too_many_arguments)]
    pub fn setting_knob(
        &mut self,
        ui: &mut Ui,
        current: f32,
        range: RangeInclusive<f32>,
        default: f32,
        name: &str,
        format: &dyn Fn(f32) -> String,
        log: bool,
    ) -> Option<f32> {
        ui.push_id(name, |ui| {
            let pending = ui.id().with("pending");
            let mut value = ui.data(|d| d.get_temp(pending)).unwrap_or(current);
            let response = ui.add(
                Knob::new(&mut value, range)
                    .log(log)
                    .default(Some(default))
                    .name(name)
                    .format(format)
                    .active(self.enabled()),
            );
            if response.dragged() {
                ui.data_mut(|d| d.insert_temp(pending, value));
            }
            if response.drag_stopped() || response.double_clicked() {
                ui.data_mut(|d| d.remove::<f32>(pending));
                return (value != current).then_some(value);
            }
            None
        })
        .inner
    }
}

/// The value before a drag of `response` started, once it ends (or on a
/// reset by double-click): time to record an undo step.
pub fn track_drag(ui: &Ui, response: &Response, before: f32) -> Option<f32> {
    let start = response.id.with("drag-start");
    if response.drag_started() {
        ui.data_mut(|d| d.insert_temp(start, before));
    }
    if response.drag_stopped() {
        return Some(ui.data_mut(|d| d.remove_temp(start)).unwrap_or(before));
    }
    response.double_clicked().then_some(before)
}

/// The controls of one kind of effect. The plugin's parameters are the
/// source of truth: read them every frame, set them when edited.
pub trait EffectEditor {
    fn ui(&mut self, ui: &mut Ui, cx: &mut EditorContext);
    /// Width of the editor's frame.
    fn width(&self) -> f32;
}

fn editor(kind: EffectKind) -> Box<dyn EffectEditor> {
    match kind {
        EffectKind::Filter => Box::new(FilterEditor),
        EffectKind::Echo => Box::new(EchoEditor),
    }
}

/// What the user did to an effect's frame.
pub struct EffectResponse {
    /// The header: drag to move.
    pub header: Response,
    /// Power button clicked.
    pub toggled: bool,
    /// Remove button clicked.
    pub removed: bool,
    pub edits: Vec<EffectEdit>,
    /// The whole frame.
    pub rect: Rect,
    /// The editor drew past the frame.
    #[cfg(test)]
    pub overflows: bool,
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

    /// The effect's frame, `height` tall: exactly, whatever its editor
    /// draws.
    pub fn effect_ui(
        &mut self,
        ui: &mut Ui,
        effect: &Effect,
        height: f32,
        sample_rate: f32,
        metrics: &AudioMetrics,
        selected: bool,
    ) -> EffectResponse {
        let editor = self
            .editors
            .entry(effect.plugin.id)
            .or_insert_with(|| editor(effect.kind));
        let theme = ui.app_theme();
        let (rect, _) = ui.allocate_exact_size(vec2(editor.width(), height), Sense::hover());
        ui.painter().rect_filled(rect, 3., theme.bg_raised);

        let clip = rect.intersect(ui.clip_rect());
        // Salted by the plugin: what its controls keep in memory, like a
        // dragged time, is its own, not shared with other effects.
        let mut frame = ui.new_child(
            UiBuilder::new()
                .id_salt(("effect", effect.plugin.id.0))
                .max_rect(rect)
                .layout(Layout::top_down(Align::Min)),
        );
        frame.set_clip_rect(clip);
        frame.spacing_mut().item_spacing = vec2(0., 0.);
        let mut response = header(&mut frame, effect, selected);

        let body = Rect::from_min_max(rect.min + vec2(0., HEADER_HEIGHT), rect.max).shrink(6.);
        let mut body_ui = ui.new_child(
            UiBuilder::new()
                .id_salt(("effect-body", effect.plugin.id.0))
                .max_rect(body)
                .layout(Layout::top_down(Align::Min)),
        );
        body_ui.set_clip_rect(body.intersect(clip));
        body_ui.spacing_mut().item_spacing = vec2(6., 4.);
        let mut cx = EditorContext {
            effect,
            sample_rate,
            metrics,
            edits: Vec::new(),
        };
        editor.ui(&mut body_ui, &mut cx);
        response.edits = cx.edits;
        #[cfg(test)]
        {
            response.overflows = !body.expand(0.5).contains_rect(body_ui.min_rect());
        }

        let stroke = if selected {
            Stroke::new(1.5, theme.accent)
        } else {
            Stroke::new(1.0, theme.border)
        };
        ui.painter()
            .rect_stroke(rect, 3., stroke, egui::StrokeKind::Inside);
        response.rect = rect;
        response
    }
}

fn header(ui: &mut Ui, effect: &Effect, selected: bool) -> EffectResponse {
    let theme = ui.app_theme();
    let enabled = effect.enabled();
    let (rect, header) = ui.allocate_exact_size(
        vec2(ui.available_width(), HEADER_HEIGHT),
        Sense::click_and_drag(),
    );
    let fill = if selected {
        theme.bg_control_hover
    } else {
        theme.bg_control
    };
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius {
            nw: 3,
            ne: 3,
            sw: 0,
            se: 0,
        },
        fill,
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink2(vec2(3., 0.)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let font = FontId::new(10., FontFamily::Name(PHOSPHOR_REGULAR.into()));
    let toggled = child
        .add(
            FlatButton::new(POWER)
                .square(15.)
                .font(font.clone())
                .selected(enabled)
                .tooltip(if enabled { "Bypass" } else { "Enable" }),
        )
        .clicked();
    let removed = child
        .with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(
                FlatButton::ghost(X)
                    .square(15.)
                    .font(font)
                    .tooltip("Remove"),
            )
            .clicked()
        })
        .inner;
    ui.painter().text(
        rect.left_center() + vec2(24., 0.),
        Align2::LEFT_CENTER,
        effect.kind.name(),
        FontId::proportional(11.),
        if enabled {
            theme.text
        } else {
            theme.text_muted
        },
    );
    EffectResponse {
        header,
        toggled,
        removed,
        edits: Vec::new(),
        rect: Rect::NOTHING,
        #[cfg(test)]
        overflows: false,
    }
}

/// `Hz` below a kilohertz, `kHz` above.
pub fn format_hz(hz: f32) -> String {
    if hz < 1000. {
        format!("{hz:.0} Hz")
    } else if hz < 10_000. {
        format!("{:.1} kHz", hz / 1000.)
    } else {
        format!("{:.0} kHz", hz / 1000.)
    }
}

pub fn format_percent(ratio: f32) -> String {
    format!("{:.0}%", ratio * 100.)
}

pub fn format_ms(seconds: f32) -> String {
    format!("{:.0} ms", seconds * 1000.)
}

/// Area for an editor's graph: what's left above a row of knobs.
pub fn graph_rect(ui: &mut Ui, sense: Sense) -> (Rect, Response) {
    let controls = Knob::HEIGHT + ui.spacing().item_spacing.y;
    let size = Vec2::new(
        ui.available_width(),
        (ui.available_height() - controls).max(0.),
    );
    ui.allocate_exact_size(size, sense)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_frequencies() {
        assert_eq!(format_hz(440.), "440 Hz");
        assert_eq!(format_hz(1500.), "1.5 kHz");
        assert_eq!(format_hz(12_000.), "12 kHz");
    }
}
