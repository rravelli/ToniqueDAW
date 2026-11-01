//! Effect editors: the UI of each [`EffectKind`], in a frame with a header
//! holding the power and collapse buttons and its name. Beside the frame,
//! low at its right, a meter of its output. A collapsed effect
//! is a strip with its name up it.

use crate::ui::font::semibold;
use crate::ui::theme::font_size;
use crate::{
    core::{
        effect::{Effect, EffectKind, Setting},
        metrics::AudioMetrics,
    },
    ui::{
        effects::{
            dynamics::{CompressorEditor, GateEditor, LimiterEditor},
            echo::EchoEditor,
            eq::EqEditor,
            filter::FilterEditor,
            reverb::ReverbEditor,
            saturation::SaturationEditor,
            spectrum::SpectrumEditor,
            tuner::TunerEditor,
            utility::UtilityEditor,
        },
        font::PHOSPHOR_REGULAR,
        theme::ThemeExt,
        widget::{
            context_menu::{ContextMenuButton, ContextMenuLabel, ContextMenuSeparator},
            flat_button::FlatButton,
            knob::Knob,
            meter::LevelMeter,
        },
    },
};
use egui::{
    Align, Align2, FontFamily, FontId, Key, Layout, Margin, Rect, Response, Sense, Stroke,
    TextEdit, Ui, UiBuilder, Vec2, epaint::TextShape, pos2, vec2,
};
use egui_phosphor::{
    fill::{COPY, TRASH},
    regular::{ARROWS_IN_SIMPLE, ARROWS_OUT_SIMPLE, CARET_DOWN, CARET_RIGHT, POWER, TEXT_T},
};
use std::f32::consts::FRAC_PI_2;
use std::{collections::HashMap, ops::RangeInclusive};
use tonique_engine::{
    edit::{Parameter, PluginId},
    param::ParamId,
};

pub mod dynamics;
pub mod echo;
pub mod eq;
pub mod filter;
pub mod graph;
pub mod reverb;
pub mod saturation;
pub mod shapes;
pub mod spectrum;
pub mod tuner;
pub mod utility;

/// Size of a parameter's switch.
pub const TOGGLE_SIZE: Vec2 = vec2(30., 20.);
const HEADER_HEIGHT: f32 = 20.;
/// Inside an effect open in a full window, and its bar.
const FULL_WINDOW_MARGIN: f32 = 12.;
const FULL_BAR_HEIGHT: f32 = 24.;
/// The output meter outside an effect's frame, low at its right, and the
/// space between them.
const METER_WIDTH: f32 = 4.;
const METER_HEIGHT: f32 = 48.;
const METER_GAP: f32 = 3.;
/// Width of a collapsed effect: a strip with its name up it.
pub const STRIP_WIDTH: f32 = 24.;

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
    /// Filled by the editor.
    pub edits: Vec<EffectEdit>,
    /// Shown in a full window, with room for more than in the rack.
    pub expanded: bool,
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

    /// Button switching a 0/1 parameter, as an undo step.
    pub fn param_toggle(&mut self, ui: &mut Ui, param: &str, name: &str, tooltip: &str) {
        let Some(p) = self.param(param) else {
            return;
        };
        let on = p.get() >= 0.5;
        let button = FlatButton::new(name)
            .size(TOGGLE_SIZE)
            .font(FontId::proportional(font_size::SMALL))
            .selected(on)
            .tooltip(tooltip);
        if ui.add(button).clicked() {
            let (old, new) = (p.get(), if on { 0. } else { 1. });
            p.set(new);
            self.edits.push(EffectEdit::Param { id: p.id, old, new });
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
            // Reset, or stepped by a key, it applies at once.
            let stepped = response.changed() && !response.dragged();
            if response.drag_stopped() || response.double_clicked() || stepped {
                ui.data_mut(|d| d.remove::<f32>(pending));
                return (value != current).then_some(value);
            }
            None
        })
        .inner
    }
}

/// The value before a drag of `response` started, once it ends (or on a
/// reset by double-click, or a step by a key): time to record an undo step.
pub fn track_drag(ui: &Ui, response: &Response, before: f32) -> Option<f32> {
    let start = response.id.with("drag-start");
    if response.drag_started() {
        ui.data_mut(|d| d.insert_temp(start, before));
    }
    if response.drag_stopped() {
        return Some(ui.data_mut(|d| d.remove_temp(start)).unwrap_or(before));
    }
    let stepped = response.changed() && !response.dragged();
    (response.double_clicked() || stepped).then_some(before)
}

/// The controls of one kind of effect. The plugin's parameters are the
/// source of truth: read them every frame, set them when edited.
pub trait EffectEditor {
    fn ui(&mut self, ui: &mut Ui, cx: &mut EditorContext);
    /// Width of the editor's frame.
    fn width(&self) -> f32;
    /// Whether it can open in a full window: a graph that's worth seeing
    /// big. It then draws at any size.
    fn expandable(&self) -> bool {
        false
    }
}

fn editor(kind: EffectKind) -> Box<dyn EffectEditor> {
    match kind {
        EffectKind::Filter => Box::new(FilterEditor::new()),
        EffectKind::Echo => Box::new(EchoEditor),
        EffectKind::Spectrum => Box::new(SpectrumEditor::new()),
        EffectKind::Tuner => Box::new(TunerEditor::new()),
        EffectKind::Utility => Box::new(UtilityEditor),
        EffectKind::Eq => Box::new(EqEditor::new()),
        EffectKind::Compressor => Box::new(CompressorEditor::new()),
        EffectKind::Limiter => Box::new(LimiterEditor::new()),
        EffectKind::Gate => Box::new(GateEditor::new()),
        EffectKind::Saturation => Box::new(SaturationEditor),
        EffectKind::Reverb => Box::new(ReverbEditor),
    }
}

/// What the user did to an effect's frame.
pub struct EffectResponse {
    /// The header (the whole strip when collapsed): drag to move.
    pub header: Response,
    /// Power button clicked.
    pub toggled: bool,
    /// Remove picked in its menu.
    pub removed: bool,
    pub duplicated: bool,
    /// Collapse or expand it.
    pub collapse: Option<bool>,
    /// Name it this (`None` for its kind's).
    pub renamed: Option<Option<String>>,
    pub edits: Vec<EffectEdit>,
    /// The whole effect: its frame, and its meter when expanded.
    pub rect: Rect,
    /// The editor drew past the frame.
    #[cfg(test)]
    pub overflows: bool,
}

impl EffectResponse {
    fn new(header: Response) -> Self {
        Self {
            header,
            toggled: false,
            removed: false,
            duplicated: false,
            collapse: None,
            renamed: None,
            edits: Vec::new(),
            rect: Rect::NOTHING,
            #[cfg(test)]
            overflows: false,
        }
    }
}

/// An effect's name being typed.
struct Renaming {
    plugin: PluginId,
    text: String,
    /// Focus was asked for: once it's gone, typing is over.
    focused: bool,
}

/// An effect's editor, and the levels its meter shows.
struct Slot {
    editor: Box<dyn EffectEditor>,
    /// What comes out of the effect.
    levels: AudioMetrics,
}

/// The editors of the effects shown, made when first shown.
#[derive(Default)]
pub struct EffectRack {
    editors: HashMap<PluginId, Slot>,
    renaming: Option<Renaming>,
    /// The effect open in a full window.
    expanded: Option<PluginId>,
    /// Where it opens; none to cover the whole window.
    full_area: Option<Rect>,
}

impl EffectRack {
    /// Editors made so far.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.editors.len()
    }

    /// Drop the editors of effects not in `shown`: an analyser's buffers
    /// are large, and removed effects' would pile up.
    pub fn retain(&mut self, shown: &[PluginId]) {
        self.editors.retain(|id, _| shown.contains(id));
        if self.expanded.is_some_and(|id| !shown.contains(&id)) {
            self.expanded = None;
        }
    }

    /// The effect open in a full window.
    #[cfg(test)]
    pub fn expanded(&self) -> Option<PluginId> {
        self.expanded
    }

    /// Start typing a new name for `effect`.
    pub fn rename(&mut self, effect: &Effect) {
        self.renaming = Some(Renaming {
            plugin: effect.plugin.id,
            text: effect.name().to_string(),
            focused: false,
        });
    }

    /// The effect's frame, `height` tall: exactly, whatever its editor
    /// draws. A strip when collapsed.
    pub fn effect_ui(
        &mut self,
        ui: &mut Ui,
        effect: &Effect,
        height: f32,
        sample_rate: f32,
        selected: bool,
    ) -> EffectResponse {
        let slot = self
            .editors
            .entry(effect.plugin.id)
            .or_insert_with(|| Slot {
                editor: editor(effect.kind),
                levels: AudioMetrics::new(),
            });
        let expandable = slot.editor.expandable();
        let full = self.expanded == Some(effect.plugin.id);
        if effect.collapsed {
            let mut response = self.strip_ui(ui, effect, height, selected, expandable);
            if full {
                self.full_window(ui.ctx(), effect, sample_rate, &mut response);
            }
            return response;
        }
        let width = slot.editor.width();
        // Its output, while shown: a bypassed effect isn't heard, so it has
        // none.
        if effect.enabled() {
            let output = &effect.plugin.tap;
            output.watch_output_for((sample_rate / 2.) as u32);
            slot.levels.update(&output.output, false);
        }
        let levels = slot.levels.clone();
        let theme = ui.app_theme();
        let (whole, _) = ui.allocate_exact_size(
            vec2(width + METER_GAP + METER_WIDTH, height),
            Sense::hover(),
        );
        let rect = Rect::from_min_size(whole.min, vec2(width, height));
        ui.painter().rect_filled(rect, 3., theme.bg_raised);
        let clip = rect.intersect(ui.clip_rect());
        let header = Rect::from_min_size(rect.min, vec2(width, HEADER_HEIGHT));
        let mut response = self.header(ui, effect, header, clip, selected, expandable);

        // Outside the frame, low down at its right.
        let meter = Rect::from_min_max(
            pos2(
                whole.right() - METER_WIDTH,
                rect.bottom() - METER_HEIGHT.min(height),
            ),
            whole.right_bottom(),
        );
        let mut meter_ui = ui.new_child(
            UiBuilder::new()
                .id_salt(("effect-meter", effect.plugin.id.0))
                .max_rect(meter),
        );
        meter_ui.set_clip_rect(meter.intersect(ui.clip_rect()));
        meter_ui.add(
            LevelMeter::new(meter.size(), &levels)
                .disabled(!effect.enabled())
                .without_clips(),
        );

        let body = Rect::from_min_max(rect.min + vec2(0., HEADER_HEIGHT), rect.max).shrink(6.);
        // Salted by the plugin: what its controls keep in memory, like a
        // dragged time, is its own, not shared with other effects.
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
            edits: Vec::new(),
            expanded: false,
        };
        if full {
            // Drawn in its window instead: one editor can't be in two places.
            let theme = ui.app_theme();
            body_ui.painter().text(
                body.center(),
                Align2::CENTER_CENTER,
                "Open in a full window",
                FontId::proportional(font_size::SMALL),
                theme.text_muted,
            );
        } else if let Some(slot) = self.editors.get_mut(&effect.plugin.id) {
            slot.editor.ui(&mut body_ui, &mut cx);
        }
        response.edits = cx.edits;
        #[cfg(test)]
        {
            response.overflows = !body.expand(0.5).contains_rect(body_ui.min_rect());
        }

        paint_outline(ui, rect, selected);
        response.rect = whole;
        if full {
            self.full_window(ui.ctx(), effect, sample_rate, &mut response);
        }
        response
    }

    /// Where an effect opens in a full window: over the effect panel and
    /// up to just under the top bar.
    pub fn set_full_area(&mut self, area: Rect) {
        self.full_area = Some(area);
    }

    /// `effect`'s editor over [`Self::set_full_area`]'s area, with a bar
    /// holding its power button, its name and a button back to the rack.
    /// Escape goes back too.
    fn full_window(
        &mut self,
        ctx: &egui::Context,
        effect: &Effect,
        sample_rate: f32,
        response: &mut EffectResponse,
    ) {
        let Some(slot) = self.editors.get_mut(&effect.plugin.id) else {
            return;
        };
        let theme = ctx.app_theme();
        let area = self
            .full_area
            .filter(|a| a.is_positive())
            .unwrap_or_else(|| ctx.content_rect());
        let shown = egui::Area::new(egui::Id::new(("effect-full-window", effect.plugin.id.0)))
            .order(egui::Order::Middle)
            .fixed_pos(area.min)
            .constrain(false)
            .show(ctx, |ui| {
                let (whole, _) = ui.allocate_exact_size(area.size(), Sense::click_and_drag());
                ui.set_clip_rect(whole);
                ui.painter().rect_filled(whole, 0., theme.bg_panel);
                let rect = whole.shrink(FULL_WINDOW_MARGIN);
                let bar = Rect::from_min_size(rect.min, vec2(rect.width(), FULL_BAR_HEIGHT));
                let mut buttons = ui.new_child(
                    UiBuilder::new()
                        .id_salt(("effect-full-bar", effect.plugin.id.0))
                        .max_rect(bar)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                buttons.spacing_mut().item_spacing.x = 8.;
                let toggled = buttons
                    .add(power_button(effect.enabled()).square(20.))
                    .clicked();
                buttons.label(
                    egui::RichText::new(effect.name())
                        .font(semibold(font_size::HEADING))
                        .color(if effect.enabled() {
                            theme.text
                        } else {
                            theme.text_muted
                        }),
                );
                let back = ui
                    .put(
                        Rect::from_min_size(
                            bar.right_top() - vec2(FULL_BAR_HEIGHT, 0.),
                            Vec2::splat(FULL_BAR_HEIGHT),
                        ),
                        FlatButton::ghost(ARROWS_IN_SIMPLE)
                            .square(FULL_BAR_HEIGHT)
                            .font(FontId::new(14., FontFamily::Name(PHOSPHOR_REGULAR.into())))
                            .tooltip("Back to the rack (Esc)"),
                    )
                    .clicked();
                let body = Rect::from_min_max(rect.min + vec2(0., FULL_BAR_HEIGHT + 8.), rect.max);
                let mut body_ui = ui.new_child(
                    UiBuilder::new()
                        .id_salt(("effect-full-body", effect.plugin.id.0))
                        .max_rect(body)
                        .layout(Layout::top_down(Align::Min)),
                );
                body_ui.set_clip_rect(body);
                body_ui.spacing_mut().item_spacing = vec2(6., 4.);
                let mut cx = EditorContext {
                    effect,
                    sample_rate,
                    edits: Vec::new(),
                    expanded: true,
                };
                slot.editor.ui(&mut body_ui, &mut cx);
                (toggled, back, cx.edits)
            });
        let (toggled, back, edits) = shown.inner;
        response.toggled |= toggled;
        response.edits.extend(edits);
        let escape =
            ctx.memory(|m| m.focused().is_none()) && ctx.input(|i| i.key_pressed(Key::Escape));
        if back || escape {
            self.expanded = None;
        }
    }

    /// Power and collapse buttons, and the name (or the field typing it).
    /// Drag it to move the effect; double-click it to collapse the effect.
    fn header(
        &mut self,
        ui: &mut Ui,
        effect: &Effect,
        rect: Rect,
        clip: Rect,
        selected: bool,
        expandable: bool,
    ) -> EffectResponse {
        let theme = ui.app_theme();
        let header = ui.interact(
            rect,
            ui.id().with(("effect-header", effect.plugin.id.0)),
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
        let mut response = EffectResponse::new(header);
        let mut buttons = ui.new_child(
            UiBuilder::new()
                .id_salt(("effect-header-buttons", effect.plugin.id.0))
                .max_rect(rect.shrink2(vec2(3., 0.)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        buttons.set_clip_rect(clip);
        buttons.spacing_mut().item_spacing.x = 2.;
        response.toggled = buttons.add(power_button(effect.enabled())).clicked();
        if buttons
            .add(icon_button(CARET_DOWN).tooltip("Collapse"))
            .clicked()
        {
            response.collapse = Some(true);
        }
        let left = buttons.cursor().left() + 2.;
        // At the right end, out of the name's way.
        let mut right = rect.right() - 4.;
        if expandable {
            let at =
                Rect::from_min_size(pos2(rect.right() - 18., rect.top() + 2.5), Vec2::splat(15.));
            if buttons
                .put(
                    at,
                    icon_button(ARROWS_OUT_SIMPLE).tooltip("Open in a full window"),
                )
                .clicked()
            {
                self.expanded = Some(effect.plugin.id);
            }
            right = at.left() - 2.;
        }

        let name = Rect::from_min_max(pos2(left, rect.top() + 2.), pos2(right, rect.bottom() - 2.));
        let mut field = ui.new_child(
            UiBuilder::new()
                .id_salt(("effect-name", effect.plugin.id.0))
                .max_rect(name),
        );
        field.set_clip_rect(name.intersect(clip));
        if !self.name_edit(&mut field, effect, name, &mut response) {
            let color = if effect.enabled() {
                theme.text
            } else {
                theme.text_muted
            };
            ui.painter().with_clip_rect(name.intersect(clip)).text(
                name.left_center(),
                Align2::LEFT_CENTER,
                effect.name(),
                semibold(font_size::LABEL),
                color,
            );
        }

        if response.header.double_clicked() {
            response.collapse = Some(true);
        }
        self.context_menu(effect, &mut response, expandable);
        response
    }

    /// A collapsed effect: its power and expand buttons, and its name up
    /// the strip. Double-click it to expand the effect.
    fn strip_ui(
        &mut self,
        ui: &mut Ui,
        effect: &Effect,
        height: f32,
        selected: bool,
        expandable: bool,
    ) -> EffectResponse {
        let theme = ui.app_theme();
        let (rect, _) = ui.allocate_exact_size(vec2(STRIP_WIDTH, height), Sense::hover());
        let clip = rect.intersect(ui.clip_rect());
        let header = ui.interact(
            rect,
            ui.id().with(("effect-strip", effect.plugin.id.0)),
            Sense::click_and_drag(),
        );
        let fill = if selected {
            theme.bg_control_hover
        } else {
            theme.bg_control
        };
        ui.painter().rect_filled(rect, 3., fill);
        let mut response = EffectResponse::new(header);
        let mut buttons = ui.new_child(
            UiBuilder::new()
                .id_salt(("effect-strip-buttons", effect.plugin.id.0))
                .max_rect(rect.shrink2(vec2(0., 3.)))
                .layout(Layout::top_down(Align::Center)),
        );
        buttons.set_clip_rect(clip);
        buttons.spacing_mut().item_spacing.y = 2.;
        response.toggled = buttons.add(power_button(effect.enabled())).clicked();
        if buttons
            .add(icon_button(CARET_RIGHT).tooltip("Expand"))
            .clicked()
        {
            response.collapse = Some(false);
        }

        // Up the strip from its bottom, reading bottom to top.
        let top = buttons.cursor().top() + 4.;
        let color = if effect.enabled() {
            theme.text
        } else {
            theme.text_muted
        };
        let galley = ui.painter().layout_no_wrap(
            effect.name().to_string(),
            semibold(font_size::LABEL),
            color,
        );
        let at = pos2(rect.center().x - galley.size().y / 2., rect.bottom() - 6.);
        let name = Rect::from_min_max(pos2(rect.left(), top), rect.max);
        ui.painter()
            .with_clip_rect(name.intersect(clip))
            .add(TextShape::new(at, galley, color).with_angle(-FRAC_PI_2));

        if response.header.double_clicked() {
            response.collapse = Some(false);
        }
        self.context_menu(effect, &mut response, expandable);
        paint_outline(ui, rect, selected);
        response.rect = rect;
        response
    }

    /// The name being typed, over `rect`, if `effect` is being renamed.
    /// Enter or clicking away keeps it; Escape drops it.
    fn name_edit(
        &mut self,
        ui: &mut Ui,
        effect: &Effect,
        rect: Rect,
        response: &mut EffectResponse,
    ) -> bool {
        let Some(renaming) = self
            .renaming
            .as_mut()
            .filter(|r| r.plugin == effect.plugin.id)
        else {
            return false;
        };
        let theme = ui.app_theme();
        let edit = ui.put(
            rect,
            TextEdit::singleline(&mut renaming.text)
                .font(FontId::proportional(font_size::LABEL))
                .background_color(theme.bg_deep)
                .text_color(theme.text)
                .margin(Margin::symmetric(2, 0)),
        );
        if !renaming.focused {
            edit.request_focus();
            crate::ui::widget::text_edit::select_all(ui.ctx(), edit.id, &renaming.text);
            renaming.focused = true;
        } else if edit.lost_focus() || !edit.has_focus() {
            if !ui.input(|i| i.key_pressed(Key::Escape)) {
                response.renamed = Some(Some(renaming.text.clone()));
            }
            self.renaming = None;
        }
        true
    }

    fn context_menu(&mut self, effect: &Effect, response: &mut EffectResponse, expandable: bool) {
        let mut rename = false;
        let mut expand = false;
        response.header.context_menu(|ui| {
            ui.add(ContextMenuLabel::new(effect.name()));
            if ui.add(ContextMenuButton::new(TEXT_T, "Rename")).clicked() {
                rename = true;
            }
            let (icon, text) = if effect.collapsed {
                (CARET_RIGHT, "Expand")
            } else {
                (CARET_DOWN, "Collapse")
            };
            if ui.add(ContextMenuButton::new(icon, text)).clicked() {
                response.collapse = Some(!effect.collapsed);
            }
            if expandable
                && ui
                    .add(ContextMenuButton::new(
                        ARROWS_OUT_SIMPLE,
                        "Open in full window",
                    ))
                    .clicked()
            {
                expand = true;
            }
            if ui.add(ContextMenuButton::new(COPY, "Duplicate")).clicked() {
                response.duplicated = true;
            }
            ui.add(ContextMenuSeparator::new());
            if ui
                .add(ContextMenuButton::new(TRASH, "Remove").text_color(ui.app_theme().danger))
                .clicked()
            {
                response.removed = true;
            }
        });
        if expand {
            self.expanded = Some(effect.plugin.id);
        }
        if rename {
            // Typed in the header: expand it first.
            if effect.collapsed {
                response.collapse = Some(false);
            }
            self.rename(effect);
        }
    }
}

fn icon_font() -> FontId {
    FontId::new(10., FontFamily::Name(PHOSPHOR_REGULAR.into()))
}

fn icon_button(icon: &str) -> FlatButton {
    FlatButton::ghost(icon).square(15.).font(icon_font())
}

fn power_button(enabled: bool) -> FlatButton {
    FlatButton::new(POWER)
        .square(15.)
        .font(icon_font())
        .selected(enabled)
        .tooltip(if enabled { "Bypass" } else { "Enable" })
}

fn paint_outline(ui: &Ui, rect: Rect, selected: bool) {
    let theme = ui.app_theme();
    let stroke = if selected {
        Stroke::new(1.5, theme.accent)
    } else {
        Stroke::new(1.0, theme.border)
    };
    ui.painter()
        .rect_stroke(rect, 3., stroke, egui::StrokeKind::Inside);
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

/// A gain in dB, signed.
pub fn format_db(db: f32) -> String {
    format!("{db:+.1} dB")
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
