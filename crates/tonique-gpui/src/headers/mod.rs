//! The track list, as Tonique's `TrackHeaders`: a panel right of the
//! timeline, resizable from its left edge and collapsible to a strip of
//! the tracks' colours and meters. A head in line with the ruler, then a
//! header per row (track or group) in line with its lane, scrolled with
//! it, each expanded group's colour bar running down to its last row; a
//! button adding a track; the master in a footer.

mod drag;
mod header;
mod layout;
mod menu;

pub use drag::Param;

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use gpui::{
    Bounds, Context, DragMoveEvent, MouseButton, Pixels, ScrollEvent, Window, canvas, div,
    prelude::*, px,
};
use tonique_engine::edit::TrackId;

use crate::{
    app::Tonique,
    bottom::DraggedEffect,
    core::{
        state::MASTER_TRACK_ID,
        track::{TrackKind, TrackRow},
    },
    geometry::Rect,
    icons,
    meter::paint_meter,
    smooth_scroll::WheelDelta,
    theme::{Theme, hsla, size},
    timeline::{FOOTER_PADDING, RULER_HEIGHT, Row, footer_height, rows},
    widgets::{Edge, Flat, SCROLLBAR_WIDTH, Splitter, TextField, glyph, splitter_handle},
};
use drag::{HeaderDrag, RowMarker};
use layout::{COLOR_BAR_WIDTH, HEADER_INSET, color_bar_left};

const DEFAULT_WIDTH: f32 = 190.;
const MIN_WIDTH: f32 = 130.;
const MAX_WIDTH: f32 = 480.;
/// Dragging the edge this much narrower than the narrowest collapses the
/// list.
const COLLAPSE_PAST: f32 = 60.;
/// Collapsed, the list is a strip of the tracks' colours and meters.
const COLLAPSED_WIDTH: f32 = 28.;
const COLLAPSED_BAR_WIDTH: f32 = 4.;
const COLLAPSED_METER_WIDTH: f32 = 10.;
/// Between the panel's edges and the headers.
const PANEL_MARGIN: f32 = 3.;
/// Right of the rows' headers: clear of the scrollbar down that edge.
const ROWS_RIGHT: f32 = SCROLLBAR_WIDTH + 1.;
const HEAD_BUTTON: f32 = 22.;
const ADD_TRACK_HEIGHT: f32 = 24.;
/// The line, or the outline, marking where a moved row would land.
const MARKER_WIDTH: f32 = 2.;
/// A row's tallest, dragged from its edge.
const MAX_ROW_HEIGHT: f32 = 400.;

/// Where each header's sliders were laid out.
type SliderBounds = Rc<RefCell<HashMap<(TrackId, Param), Bounds<Pixels>>>>;

pub struct HeadersUi {
    /// The list's, as last laid out.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// Expanded: how wide.
    width: Splitter,
    /// Expanded, or collapsed to a strip.
    open: bool,
    /// The sliders, as last laid out, for their drags.
    sliders: SliderBounds,
    drag: Option<HeaderDrag>,
    /// The track being renamed, and its new name so far.
    rename: Option<(TrackId, TextField)>,
}

impl HeadersUi {
    pub fn new() -> Self {
        Self {
            bounds: Rc::default(),
            width: Splitter::new(Edge::Left, DEFAULT_WIDTH, MIN_WIDTH, MAX_WIDTH),
            open: true,
            sliders: Rc::default(),
            drag: None,
            rename: None,
        }
    }

    /// What referred to a replaced project's tracks.
    pub fn forget_project(&mut self) {
        self.drag = None;
        self.rename = None;
    }

    /// Expanded, or collapsed to a strip.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Expands or collapses the list.
    pub fn toggle_list(&mut self) {
        self.open = !self.open;
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some() || self.width.is_dragging()
    }
}

impl Tonique {
    /// The row of track or group `id`, the master's included.
    pub fn track_row(&self, id: TrackId) -> Option<TrackRow> {
        if id == MASTER_TRACK_ID {
            return Some(self.state.master_track());
        }
        if self.state.is_group(id) {
            return self.state.groups().into_iter().find(|group| group.id == id);
        }
        self.state.tracks().find(|candidate| candidate.id == id)
    }

    /// The track list's, as last laid out.
    pub fn headers_bounds(&self) -> Bounds<Pixels> {
        self.headers.bounds.get()
    }

    pub fn headers_view(&mut self, context: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let open = self.headers.open;
        let width = if open {
            self.headers.width.size
        } else {
            COLLAPSED_WIDTH
        };
        let list = if open {
            self.header_list(&theme, context).into_any_element()
        } else {
            self.collapsed_list(&theme, context).into_any_element()
        };
        let slot = self.headers.bounds.clone();
        div()
            .id("track-list")
            .w(px(width))
            .h_full()
            .flex_none()
            .relative()
            .overflow_hidden()
            .bg(hsla(theme.bg_panel))
            .on_scroll(context.listener(|this, event: &ScrollEvent, _, context| {
                this.scroll_rows_by_wheel(WheelDelta::of(event.delta));
                context.notify();
            }))
            .on_drag_move(context.listener(
                |this, event: &DragMoveEvent<DraggedEffect>, _, context| {
                    this.follow_effect_drag(event.event.position);
                    context.notify();
                },
            ))
            .on_drop(
                context.listener(|this, dragged: &DraggedEffect, window, context| {
                    this.drop_effect(dragged, window.mouse_position());
                    context.notify();
                }),
            )
            .child(
                canvas(move |bounds, _, _| slot.set(bounds), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            )
            .child(list)
            .child(self.rows_scrollbar(context))
            .when(open, |element| element.child(self.row_marker(&theme)))
            // Over the rows, in line with the timeline's footer.
            .child(self.list_footer(open, &theme, context))
            // Over the rows, in line with the ruler.
            .child(self.list_head(open, &theme, context))
            .when(open, |element| {
                element.child(splitter_handle(
                    "track-list-edge",
                    &self.headers.width,
                    &theme,
                    context,
                    |this| &mut this.headers.width,
                ))
            })
    }

    /// Where a row being moved would land: a line between rows, or a
    /// group's header outlined.
    fn row_marker(&self, theme: &Theme) -> impl IntoElement {
        let marker = self.row_drop().map(|(_, _, marker)| marker);
        let accent = theme.accent;
        let footer = footer_height(&self.state);
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let Some(marker) = marker else {
                    return;
                };
                let area = Rect::of(bounds);
                let (left, width) = (
                    area.left + PANEL_MARGIN,
                    area.width - PANEL_MARGIN - ROWS_RIGHT,
                );
                let quad = match marker {
                    RowMarker::Line { y } => {
                        // Between the head and the footer, which hide what's
                        // under them: the first gap is under the head.
                        let (lowest, highest) = (RULER_HEIGHT, area.height - footer);
                        let top = (y - MARKER_WIDTH / 2.)
                            .min(highest - MARKER_WIDTH)
                            .max(lowest);
                        gpui::fill(
                            Rect::new(left, area.top + top, width, MARKER_WIDTH).bounds(),
                            hsla(accent),
                        )
                    }
                    RowMarker::Into { top, height } => gpui::outline(
                        Rect::new(left, area.top + top, width, height).bounds(),
                        hsla(accent),
                        gpui::BorderStyle::Solid,
                    )
                    .border_widths(px(MARKER_WIDTH))
                    .corner_radii(px(2.)),
                };
                window.paint_quad(quad);
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// Expand and collapse every row, and collapse the list; or, collapsed,
    /// expand it.
    fn list_head(
        &self,
        open: bool,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let button = |id: &'static str, glyph_name: &'static str| {
            Flat::ghost(id, HEAD_BUTTON).element(glyph(glyph_name, 14., false), theme)
        };
        let toggle = if open {
            icons::CARET_DOUBLE_RIGHT
        } else {
            icons::CARET_DOUBLE_LEFT
        };
        let toggle = button("toggle-list", toggle).on_mouse_down(
            MouseButton::Left,
            context.listener(|this, _, _, context| {
                context.stop_propagation();
                this.headers.toggle_list();
                context.notify();
            }),
        );
        let all = |id, glyph_name, collapsed: bool, context: &mut Context<Self>| {
            button(id, glyph_name).on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, _, _, context| {
                    context.stop_propagation();
                    this.state.set_all_collapsed(collapsed);
                    context.notify();
                }),
            )
        };
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(RULER_HEIGHT))
            .flex()
            .items_center()
            .gap(px(2.))
            .px(px(PANEL_MARGIN))
            .bg(hsla(theme.bg_panel))
            .border_b_1()
            .border_color(hsla(theme.border))
            .when(open, |element| {
                element
                    .child(all(
                        "expand-all",
                        icons::ARROWS_OUT_LINE_VERTICAL,
                        false,
                        context,
                    ))
                    .child(all(
                        "collapse-all",
                        icons::ARROWS_IN_LINE_VERTICAL,
                        true,
                        context,
                    ))
                    .child(div().flex_1())
            })
            .child(toggle)
    }

    /// The master, in a footer below the rows: its header, or collapsed,
    /// its colour and meter.
    fn list_footer(
        &self,
        open: bool,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let master = self.state.master_track();
        let footer = div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .h(px(footer_height(&self.state)))
            .pt(px(FOOTER_PADDING))
            .bg(hsla(theme.bg_panel))
            .border_t_1()
            .border_color(hsla(theme.border));
        if open {
            let width = self.headers.width.size - 2. * PANEL_MARGIN;
            footer
                .px(px(PANEL_MARGIN))
                .child(self.header(master, width, theme, context))
        } else {
            footer.child(self.collapsed_row(master, theme, context))
        }
    }

    /// A header per row, then the button adding a track; below, a click
    /// clears the selection.
    fn header_list(&self, theme: &Theme, context: &mut Context<Self>) -> impl IntoElement {
        let width = self.headers.width.size - PANEL_MARGIN - ROWS_RIGHT;
        let rows = rows(&self.state);
        let scopes = group_scopes(&rows, theme);
        let mut list = div()
            .absolute()
            .top(px(RULER_HEIGHT - self.state.grid.offset.y))
            .left(px(PANEL_MARGIN))
            .right(px(ROWS_RIGHT))
            .flex()
            .flex_col();
        for row in rows {
            list = list.child(self.header(row.track, width, theme, context));
        }
        // Over the headers, the bars of the expanded groups down to their
        // last rows.
        let list = list.child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let area = Rect::of(bounds);
                    for (bar, color) in &scopes {
                        let bar = Rect::new(
                            area.left + bar.left,
                            area.top + bar.top,
                            bar.width,
                            bar.height,
                        );
                        window.paint_quad(gpui::fill(bar.bounds(), hsla(*color)));
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        );
        let add = Flat::ghost("add-track", ADD_TRACK_HEIGHT)
            .full_width()
            .color(theme.text_muted)
            .show(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(px(size::LABEL))
                    .child(glyph(icons::PLUS, size::LABEL, false))
                    .child("Add track"),
                theme,
                context,
                |this, _| {
                    this.state.add_track();
                },
            );
        let below = div()
            .id("below-tracks")
            .flex_1()
            .min_h(px(200.))
            .on_mouse_down(
                MouseButton::Left,
                context.listener(|this, _, _, context| {
                    this.state.clear_track_selection();
                    context.notify();
                }),
            );
        list.child(add).child(below)
    }

    /// Collapsed: each row a bar of its colour and its meter; clicking one
    /// selects it.
    fn collapsed_list(&self, theme: &Theme, context: &mut Context<Self>) -> impl IntoElement {
        let mut list = div()
            .absolute()
            .top(px(RULER_HEIGHT - self.state.grid.offset.y))
            .left_0()
            .right_0()
            .flex()
            .flex_col();
        for row in rows(&self.state) {
            list = list.child(self.collapsed_row(row.track, theme, context));
        }
        list
    }

    fn collapsed_row(
        &self,
        track: TrackRow,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = track.id;
        let silenced = track.is_silenced();
        let color = if silenced {
            theme.text_disabled
        } else {
            track.color
        };
        let metrics = self.state.metrics.tracks.get(&id).cloned();
        let canvas_theme = theme.clone();
        let inset = 4.;
        div()
            .id(("collapsed-row", id.0 as usize))
            .flex_none()
            .w_full()
            .h(px(track.height + crate::timeline::ROW_GAP))
            .flex()
            .justify_center()
            .gap(px(3.))
            .py(px(inset))
            .rounded(px(2.))
            .when(track.selected, |element| {
                element.bg(hsla(theme.hover_overlay))
            })
            .hover(|style| style.bg(hsla(theme.hover_overlay)))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, _, _, context| {
                    this.state.select_track(&id);
                    context.notify();
                }),
            )
            .child(
                div()
                    .w(px(COLLAPSED_BAR_WIDTH))
                    .h(px(track.height - 2. * inset))
                    .rounded(px(1.))
                    .bg(hsla(color)),
            )
            .child(
                div()
                    .w(px(COLLAPSED_METER_WIDTH))
                    .h(px(track.height - 2. * inset))
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window: &mut Window, _| {
                                if let Some(metrics) = &metrics {
                                    paint_meter(
                                        window,
                                        Rect::of(bounds),
                                        metrics,
                                        &canvas_theme,
                                        silenced,
                                    );
                                }
                            },
                        )
                        .size_full(),
                    ),
            )
    }
}

/// Each expanded group's colour bar, extended down to its last row so its
/// scope shows: local to the list, and in its colour.
fn group_scopes(rows: &[Row], theme: &Theme) -> Vec<(Rect, crate::theme::Color)> {
    rows.iter()
        .filter(|row| {
            let group = &row.track;
            // Only expanded groups with something inside.
            group.kind == TrackKind::Group
                && !group.collapsed
                && row.scope_bottom > row.top + group.height
        })
        .map(|row| {
            let group = &row.track;
            let top = row.top + HEADER_INSET;
            let bar = Rect::new(
                color_bar_left(group.depth),
                top,
                COLOR_BAR_WIDTH,
                row.scope_bottom - HEADER_INSET - top,
            );
            let color = if group.is_silenced() {
                theme.text_disabled
            } else {
                group.color
            };
            (bar, color)
        })
        .collect()
}
