//! A track's or a group's header, as Tonique's `TrackHeader`: indented by
//! the groups it's in, selected by a click,
//! collapsed by its caret, muted and soloed, its volume and pan dragged,
//! renamed in place, its menu on a right-click, and its height dragged
//! from its bottom edge.

use crate::model::HoldsProject;
use gpui::{
    Anchor, AnyElement, Context, CursorStyle, MouseButton, MouseDownEvent, Stateful, canvas, div,
    prelude::*, px,
};
use tonique_engine::edit::TrackId;

use crate::{
    app::{ArrowHolder, PopupKind, Tonique},
    core::{
        state::MASTER_TRACK_ID,
        track::{TrackRow, TrackSoloState},
    },
    geometry::Rect,
    headers::{drag::Param, layout::HeaderLayout},
    icons,
    meter::paint_meter,
    theme::{Color, Theme, hsla, icon_fill, size, with_alpha},
    timeline::{DROP_FILL_ALPHA, DROP_OUTLINE, ROW_GAP},
    utils::display_name,
    widgets::paint_param_slider,
};

/// Wide enough for the volume to show its unit.
const VOLUME_UNIT_WIDTH: f32 = 64.;
/// How opaque a solo is on a track silenced anyway.
const SILENCED_SOLO_ALPHA: u8 = 90;

/// An absolutely placed box over `rect`, local to the header.
fn place(rect: Rect) -> gpui::Div {
    div()
        .absolute()
        .left(px(rect.left))
        .top(px(rect.top))
        .w(px(rect.width))
        .h(px(rect.height))
}

/// Mute or solo: filled while on, outlined while off.
fn toggle(
    id: &'static str,
    label: &'static str,
    on: Option<Color>,
    rect: Rect,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let element = place(rect)
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(2.))
        .text_size(px(size::SMALL));
    match on {
        Some(color) => element
            .bg(hsla(color))
            .text_color(hsla(theme.text_on(color))),
        None => element
            .border_1()
            .border_color(hsla(theme.border))
            .text_color(hsla(theme.text_muted)),
    }
    .hover(|style| style.opacity(0.85))
    .cursor_pointer()
    .child(label)
}

/// Over the header of a row an effect would be dropped on, lit and
/// outlined as its lane is in the timeline.
fn drop_highlight(theme: &Theme) -> gpui::Div {
    div()
        .absolute()
        .inset_0()
        .rounded(px(2.))
        .bg(hsla(with_alpha(theme.accent, DROP_FILL_ALPHA)))
        .border(px(DROP_OUTLINE))
        .border_color(hsla(theme.accent))
}

impl Tonique {
    /// The header of `track`, `width` wide, and below it the edge that
    /// resizes it.
    pub(super) fn header(
        &self,
        track: &TrackRow,
        width: f32,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let id = track.id;
        let master = id == MASTER_TRACK_ID;
        let layout = HeaderLayout::new(width, track.height, track.depth, track.collapsed, !master);
        let silenced = track.is_silenced();
        let fill = if track.selected {
            theme.bg_control
        } else {
            theme.bg_raised
        };
        let bar = if silenced {
            theme.text_disabled
        } else {
            track.color
        };
        let mut header = div()
            .id(("header", id.0 as usize))
            .relative()
            .flex_none()
            .w_full()
            .h(px(track.height))
            .bg(hsla(fill))
            .rounded(px(2.))
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, event: &MouseDownEvent, _, context| {
                    // A double-click shows its effects, or hides them.
                    if event.click_count == 2 {
                        let was_selected = this.state(context).selected_tracks().contains(&id);
                        this.state_mut(context).select_track(&id);
                        this.bottom.update(context, |bottom, context| {
                            bottom.toggle_effects_of_selected(was_selected);
                            context.notify();
                        });
                        context.notify();
                        return;
                    }
                    // Ctrl+click adds to the selection, Shift+click selects a range.
                    if event.modifiers.control || event.modifiers.platform {
                        this.state_mut(context).toggle_track_selected(id);
                    } else if event.modifiers.shift {
                        this.state_mut(context).select_rows_to(id);
                    } else {
                        this.state_mut(context).select_track(&id);
                    }
                    // Dragged a little, it moves; the master stays in its footer.
                    if !master {
                        this.row_down(id, event);
                    }
                    context.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                context.listener(move |this, event: &MouseDownEvent, _, context| {
                    this.state_mut(context).select_track(&id);
                    this.toggle_popup(
                        PopupKind::Track(id),
                        event.position,
                        Anchor::TopLeft,
                        context,
                    );
                    context.notify();
                }),
            )
            .child(place(layout.bar).rounded(px(1.)).bg(hsla(bar)))
            .child(self.caret(track, layout.caret, theme, context))
            .child(self.name(track, layout.name, theme, context))
            .children(self.toggles(track, &layout, theme, context))
            .child(self.meter(id, silenced, layout.meter, theme, context))
            // An effect dragged over it would go on it.
            .when(self.timeline.effect_target == Some(id), |element| {
                element.child(drop_highlight(theme))
            })
            // Being moved.
            .when(self.moving_row() == Some(id), |element| {
                element.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(2.))
                        .bg(hsla(theme.hover_overlay)),
                )
            });
        if let Some((volume, pan)) = layout.sliders {
            header = header
                .child(self.slider(track, Param::Volume, volume, theme, context))
                .child(self.slider(track, Param::Pan, pan, theme, context));
        }
        // The master's height isn't dragged: it sits in a footer.
        div()
            .flex_none()
            .w_full()
            .child(header)
            .when(!master, |element| {
                element.child(self.row_edge(track, context))
            })
            .into_any_element()
    }

    fn caret(
        &self,
        track: &TrackRow,
        rect: Rect,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let (id, collapsed) = (track.id, track.collapsed);
        let glyph = if collapsed {
            icons::CARET_RIGHT
        } else {
            icons::CARET_DOWN
        };
        place(rect)
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(size::SMALL))
            .text_color(hsla(theme.text_muted))
            .cursor_pointer()
            .child(icon_fill(glyph))
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, _, _, context| {
                    context.stop_propagation();
                    this.state_mut(context).set_collapsed(&id, !collapsed);
                    context.notify();
                }),
            )
    }

    /// The name, or the field renaming it.
    fn name(
        &self,
        track: &TrackRow,
        rect: Rect,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let element = place(rect)
            .flex()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_size(px(size::LABEL));
        match self.rename_field(track.id) {
            Some(field) => element
                .child(self.rename_view(field, theme, context))
                .into_any_element(),
            None => element
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(hsla(theme.text))
                .child(display_name(&track.name, track.first_track_index))
                .into_any_element(),
        }
    }

    /// Mute in the warning colour; solo, where there's one, in the
    /// track's (fainter when silenced by another solo), as Tonique's
    /// toggles.
    fn toggles(
        &self,
        track: &TrackRow,
        layout: &HeaderLayout,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> Vec<Stateful<gpui::Div>> {
        let id = track.id;
        let soloed = matches!(track.solo, TrackSoloState::Solo);
        let solo_fill = match (soloed, track.is_silenced()) {
            (false, _) => None,
            (true, false) => Some(track.color),
            (true, true) => Some(with_alpha(track.color, SILENCED_SOLO_ALPHA)),
        };
        let muted = track.muted;
        let mute = toggle(
            "mute",
            "M",
            muted.then_some(theme.warning),
            layout.mute,
            theme,
        )
        .on_mouse_down(
            MouseButton::Left,
            context.listener(move |this, _, _, context| {
                context.stop_propagation();
                this.state_mut(context).set_mute(id, !muted);
                context.notify();
            }),
        );
        let solo = layout.solo.map(|rect| {
            toggle("solo", "S", solo_fill, rect, theme).on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, event: &MouseDownEvent, _, context| {
                    context.stop_propagation();
                    // Shift: this one alone.
                    this.state_mut(context)
                        .toggle_solo(id, event.modifiers.shift);
                    context.notify();
                }),
            )
        });
        std::iter::once(mute).chain(solo).collect()
    }

    /// What the track plays. Clicked, its clip lamps are reset.
    fn meter(
        &self,
        id: TrackId,
        silenced: bool,
        rect: Rect,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let metrics = self.state(context).metrics.tracks.get(&id).cloned();
        let canvas_theme = theme.clone();
        place(rect)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, _, _, context| {
                    context.stop_propagation();
                    this.state_mut(context).reset_meter_clip(id);
                    context.notify();
                }),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        if let Some(metrics) = &metrics {
                            paint_meter(window, Rect::of(bounds), metrics, &canvas_theme, silenced);
                        }
                    },
                )
                .size_full(),
            )
    }

    /// The volume or the pan: dragged, or double-clicked back to its
    /// default.
    fn slider(
        &self,
        track: &TrackRow,
        param: Param,
        rect: Rect,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = track.id;
        let slot = self.headers.sliders.clone();
        let canvas_theme = theme.clone();
        let (value, color) = (param.of(track), track.color);
        let unit = param == Param::Volume && rect.width >= VOLUME_UNIT_WIDTH;
        let focused = crate::app::arrows::holds(&ArrowHolder::Header(id, param), context);
        place(rect)
            .cursor(CursorStyle::ResizeLeftRight)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, event: &MouseDownEvent, _, context| {
                    context.stop_propagation();
                    this.slider_down(id, param, event, context);
                    context.notify();
                }),
            )
            .child(
                canvas(
                    move |bounds, _, _| {
                        slot.borrow_mut().insert((id, param), bounds);
                    },
                    move |bounds, _, window, context| {
                        let scale = param.scale();
                        paint_param_slider(
                            window,
                            context,
                            Rect::of(bounds),
                            scale,
                            value,
                            color,
                            (unit, focused),
                            &canvas_theme,
                        )
                    },
                )
                .size_full(),
            )
    }

    /// The gap below a row: dragged, the row's height.
    fn row_edge(&self, track: &TrackRow, context: &mut Context<Self>) -> impl IntoElement {
        let (id, height) = (track.id, track.height);
        let edge = div().w_full().h(px(ROW_GAP));
        if track.collapsed {
            return edge;
        }
        edge.cursor(CursorStyle::ResizeUpDown).on_mouse_down(
            MouseButton::Left,
            context.listener(move |this, event: &MouseDownEvent, _, context| {
                context.stop_propagation();
                this.row_edge_down(id, height, event);
                context.notify();
            }),
        )
    }
}
