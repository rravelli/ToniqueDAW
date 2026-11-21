//! An effect's frame: a header holding its power and collapse buttons and
//! its name, its editor below, and beside it, low at its right, a meter of
//! its output. A collapsed effect is a strip with its name down it. The
//! header (the whole strip) is dragged to move the effect, double-clicked
//! to fold it, and right-clicked for its menu.

use gpui::{
    Anchor, AnyElement, Context, CursorStyle, MouseButton, MouseDownEvent, Stateful, Window,
    canvas, div, prelude::*, px,
};
use tonique_engine::edit::{PluginId, TrackId};

use crate::{
    app::{PopupKind, Tonique},
    bottom::rack::{RackDrag, editors::Editor, editors::EditorInput},
    core::{effect::Effect, metrics::AudioMetrics},
    geometry::Rect,
    icons,
    meter::paint_meter,
    theme::{Theme, hsla, size},
    widgets::{Flat, Menu, MenuItem, TextField, glyph, typing_view},
};

const HEADER_HEIGHT: f32 = 20.;
/// The output meter outside the frame, low at its right, and the space
/// between them.
const METER_WIDTH: f32 = 4.;
const METER_HEIGHT: f32 = 48.;
const METER_GAP: f32 = 3.;
/// Width of a collapsed effect.
const STRIP_WIDTH: f32 = 24.;
const BUTTON: f32 = 15.;
const ICON_SIZE: f32 = 10.;

impl Tonique {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn effect_frame(
        &self,
        index: usize,
        input: &EditorInput,
        levels: AudioMetrics,
        selected: bool,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let effect = input.effect;
        let width = Editor::width(effect.kind);
        let enabled = effect.enabled();
        let header = self.effect_header(index, effect, input.track, selected, theme, context);
        let body = div()
            .flex_1()
            .min_h_0()
            .p(px(6.))
            .child(self.editor_body(input, theme, context));
        let frame = div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(width))
            .h_full()
            .rounded(px(3.))
            .overflow_hidden()
            .bg(hsla(theme.bg_raised))
            .border_1()
            .border_color(hsla(if selected { theme.accent } else { theme.border }))
            .child(header)
            .child(body);
        let meter_theme = theme.clone();
        let meter = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                paint_meter(window, Rect::of(bounds), &levels, &meter_theme, !enabled)
            },
        )
        .flex_none()
        .w(px(METER_WIDTH))
        .h(px(METER_HEIGHT));
        self.recorded(index, effect.plugin.id)
            .flex()
            .items_end()
            .gap(px(METER_GAP))
            .h_full()
            .child(frame)
            .child(meter)
            .into_any_element()
    }

    /// A collapsed effect: its power and expand buttons, and its name
    /// down the strip.
    pub(super) fn effect_strip(
        &self,
        index: usize,
        effect: &Effect,
        track: TrackId,
        selected: bool,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let plugin = effect.plugin.id;
        let color = if effect.enabled() {
            theme.text
        } else {
            theme.text_muted
        };
        let letters = effect
            .name()
            .chars()
            .map(|letter| div().h(px(size::LABEL + 1.)).child(letter.to_string()));
        let strip = div()
            .id("strip")
            .flex()
            .flex_col()
            .items_center()
            .gap(px(2.))
            .pt(px(3.))
            .w(px(STRIP_WIDTH))
            .h_full()
            .overflow_hidden()
            .rounded(px(3.))
            .bg(hsla(if selected {
                theme.bg_control_hover
            } else {
                theme.bg_control
            }))
            .border_1()
            .border_color(hsla(if selected { theme.accent } else { theme.border }))
            .cursor(CursorStyle::OpenHand)
            .child(self.power_button(effect, track, theme, context))
            .child(Flat::ghost("expand", BUTTON).show(
                glyph(icons::CARET_RIGHT, ICON_SIZE, false),
                theme,
                context,
                move |this, _| this.state.set_effect_collapsed(plugin, false),
            ))
            .child(
                div()
                    .pt(px(4.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .text_size(px(size::LABEL))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(hsla(color))
                    .children(letters),
            );
        let strip = self.grabbed(strip, index, plugin, track, true, context);
        self.recorded(index, plugin)
            .h_full()
            .child(strip)
            .into_any_element()
    }

    /// The effect's element, its bounds kept for selecting and dropping.
    fn recorded(&self, index: usize, plugin: PluginId) -> Stateful<gpui::Div> {
        let frames = self.bottom.rack.frames.clone();
        div()
            .id(("effect", plugin.0 as usize))
            .relative()
            .flex_none()
            .child(
                canvas(
                    move |bounds, _, _| {
                        if let Some(slot) = frames.borrow_mut().get_mut(index) {
                            *slot = bounds;
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
    }

    /// `element` dragged to move the effect, double-clicked to fold it
    /// (`collapsed` or not), right-clicked for its menu.
    fn grabbed(
        &self,
        element: Stateful<gpui::Div>,
        index: usize,
        plugin: PluginId,
        track: TrackId,
        collapsed: bool,
        context: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        element
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, event: &MouseDownEvent, _, context| {
                    if event.click_count >= 2 {
                        this.bottom.rack.drag = None;
                        this.state.set_effect_collapsed(plugin, !collapsed);
                    } else {
                        let pointer_x = f32::from(event.position.x);
                        this.bottom.rack.drag = Some(RackDrag::Move {
                            plugin,
                            from: index,
                            start_x: pointer_x,
                            last_x: pointer_x,
                            moved: false,
                        });
                    }
                    context.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                context.listener(move |this, event: &MouseDownEvent, _, context| {
                    context.stop_propagation();
                    this.toggle_popup(
                        PopupKind::Effect(track, plugin),
                        event.position,
                        Anchor::TopLeft,
                    );
                    context.notify();
                }),
            )
    }

    fn power_button(
        &self,
        effect: &Effect,
        track: TrackId,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let (plugin, enabled) = (effect.plugin.id, effect.enabled());
        Flat::new("power", BUTTON).selected(enabled).show(
            glyph(icons::POWER, ICON_SIZE, false),
            theme,
            context,
            move |this, _| this.state.set_effect_enabled(&track, plugin, !enabled),
        )
    }

    /// Power and collapse buttons, and the name (or the field typing it).
    fn effect_header(
        &self,
        index: usize,
        effect: &Effect,
        track: TrackId,
        selected: bool,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let plugin = effect.plugin.id;
        let color = if effect.enabled() {
            theme.text
        } else {
            theme.text_muted
        };
        let name = match &self.bottom.rack.renaming {
            Some((_, renaming, field)) if *renaming == plugin => typing_view(
                "effect-rename",
                field,
                theme,
                context,
                |this| this.bottom.rack.renaming.as_mut().map(|(.., field)| field),
                Self::finish_effect_rename,
            )
            .h(px(HEADER_HEIGHT - 4.))
            .into_any_element(),
            _ => div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(px(size::LABEL))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(hsla(color))
                .child(effect.name().to_string())
                .into_any_element(),
        };
        let header = div()
            .id("effect-header")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .h(px(HEADER_HEIGHT))
            .px(px(3.))
            .bg(hsla(if selected {
                theme.bg_control_hover
            } else {
                theme.bg_control
            }))
            .cursor(CursorStyle::OpenHand)
            .child(self.power_button(effect, track, theme, context))
            .child(Flat::ghost("collapse", BUTTON).show(
                glyph(icons::CARET_DOWN, ICON_SIZE, false),
                theme,
                context,
                move |this, _| this.state.set_effect_collapsed(plugin, true),
            ))
            .child(div().flex_1().min_w_0().ml(px(2.)).mr(px(1.)).child(name));
        self.grabbed(header, index, plugin, track, false, context)
    }

    /// Starts typing a new name for an effect, expanding it first: the name
    /// is typed in its header.
    pub(super) fn rename_effect(
        &mut self,
        track: TrackId,
        plugin: PluginId,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let Some(effect) = self
            .state
            .effects(&track)
            .into_iter()
            .find(|effect| effect.plugin.id == plugin)
        else {
            return;
        };
        if effect.collapsed {
            self.state.set_effect_collapsed(plugin, false);
        }
        let mut field = TextField::new(context);
        field.start(effect.name().to_string(), window, context);
        self.bottom.rack.renaming = Some((track, plugin, field));
    }

    fn finish_effect_rename(
        &mut self,
        keep: bool,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let Some((track, plugin, field)) = self.bottom.rack.renaming.take() else {
            return;
        };
        if keep {
            self.state
                .rename_effect(&track, plugin, Some(field.text.trim().to_string()));
        }
        window.focus(&self.focus, context);
    }

    /// An effect's menu, while it exists.
    pub fn effect_menu(
        &self,
        track: TrackId,
        plugin: PluginId,
        context: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let effects = self.state.effects(&track);
        let index = effects
            .iter()
            .position(|effect| effect.plugin.id == plugin)?;
        let effect = &effects[index];
        let theme = &self.theme;
        let collapsed = effect.collapsed;
        let (icon, text) = if collapsed {
            (icons::CARET_RIGHT, "Expand")
        } else {
            (icons::CARET_DOWN, "Collapse")
        };
        Some(
            Menu::new(theme)
                .label(effect.name().to_string())
                .item(
                    MenuItem::new("Rename").icon(icons::TEXT_T),
                    context,
                    move |this, window, context| this.rename_effect(track, plugin, window, context),
                )
                .item(
                    MenuItem::new(text).icon(icon),
                    context,
                    move |this, _, _| this.state.set_effect_collapsed(plugin, !collapsed),
                )
                .item(
                    MenuItem::new("Duplicate").icon(icons::COPY),
                    context,
                    move |this, _, _| {
                        this.bottom.rack.selected = this.state.duplicate_effects(&track, &[index]);
                    },
                )
                .separator()
                .item(
                    MenuItem::new("Remove")
                        .icon(icons::TRASH)
                        .color(theme.danger),
                    context,
                    move |this, _, _| {
                        this.state.remove_effects(&track, &[index]);
                        this.bottom.rack.selected.clear();
                    },
                )
                .into_element(),
        )
    }
}
