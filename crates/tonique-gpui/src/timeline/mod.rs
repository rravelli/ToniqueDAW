//! The arrangement: the ruler (loop lane, bars, time), a lane per track with
//! its clips, the grid, the playhead and the edit cursor. Edits go through
//! Tonique's `ProjectState` and `GridService`, as in the egui app.
//!
//! Each frame, `timeline_view` takes what's shown out of the state into a
//! [`scene::Scene`]; the canvas paints it ([`paint`]). Pointer handling is
//! on the wrapping div ([`input`], [`drop`]); drags are then followed by
//! the root view, so they work outside it.

mod clip;
mod drop;
mod expand;
mod footer;
mod input;
mod menus;
mod moving;
mod paint;
mod rows;
mod ruler;
mod scene;
mod scroll;

use std::{cell::Cell, rc::Rc};

use gpui::{
    Bounds, ContentMask, Context, CursorStyle, Modifiers, MouseButton, Pixels, Point, Window,
    canvas, div, prelude::*, px,
};

pub use drop::DropTarget;
pub use rows::{FOOTER_PADDING, Row, footer_height, rows, rows_height};

use tonique_engine::edit::TrackId;

use crate::{
    app::Tonique,
    widgets::{Axis, SCROLLBAR_WIDTH, scrollbar},
};
use input::TimelineDrag;
use rows::ClipPart;
use ruler::LoopGrab;

pub const LOOP_HEIGHT: f32 = 12.;
pub const BARS_HEIGHT: f32 = 20.;
pub const TIME_HEIGHT: f32 = 16.;
pub const RULER_HEIGHT: f32 = LOOP_HEIGHT + BARS_HEIGHT + TIME_HEIGHT;
/// Between rows, as `ROW_GAP` in Tonique's track headers.
pub const ROW_GAP: f32 = 3.;
/// How opaque a row an effect would be dropped on is lit, its lane and
/// its header, and how thick its outline is.
pub const DROP_FILL_ALPHA: u8 = 40;
pub const DROP_OUTLINE: f32 = 1.5;

#[derive(Default)]
pub struct TimelineUi {
    pub bounds: Rc<Cell<Bounds<Pixels>>>,
    drag: TimelineDrag,
    /// Where the pointer was last during a drag, in the window, and the
    /// modifiers held: followed again as the timeline scrolls under it.
    pointer: Option<(Point<Pixels>, Modifiers)>,
    /// Pointer over the timeline, local to it.
    hover: Option<(f32, f32)>,
    /// Alt held as the pointer last moved over it: snapping inverted.
    hover_alt: bool,
    pub drop_target: Option<DropTarget>,
    /// The track an effect dragged over its lane or header would go on.
    pub effect_target: Option<TrackId>,
    /// Collapsed groups opened by dragging over them, to close again once
    /// the pointer leaves them.
    hover_expanded: Vec<TrackId>,
    scroll: scroll::TimelineScroll,
}

impl TimelineUi {
    /// What referred to a replaced project's clips and tracks.
    pub fn forget_project(&mut self) {
        self.drag = TimelineDrag::None;
        self.pointer = None;
        self.drop_target = None;
        self.effect_target = None;
        self.hover_expanded.clear();
    }

    pub fn is_dragging(&self) -> bool {
        !matches!(self.drag, TimelineDrag::None)
    }
}

impl Tonique {
    fn cursor(&self) -> CursorStyle {
        match &self.timeline.drag {
            TimelineDrag::Move(press) if press.dragged.is_some() => return CursorStyle::ClosedHand,
            TimelineDrag::Zone { moved: true, .. } => return CursorStyle::IBeam,
            TimelineDrag::Trim { .. } | TimelineDrag::Scrub => return CursorStyle::ResizeLeftRight,
            TimelineDrag::Loop { grab, .. } => {
                return match grab {
                    LoopGrab::Move { .. } => CursorStyle::ClosedHand,
                    _ => CursorStyle::ResizeLeftRight,
                };
            }
            _ => {}
        }
        let Some((local_x, local_y)) = self.timeline.hover else {
            return CursorStyle::Arrow;
        };
        if local_y < LOOP_HEIGHT && !self.on_playhead_head(local_x, local_y) {
            return match self.loop_grab_at(local_x) {
                Some(LoopGrab::Start | LoopGrab::End) => CursorStyle::ResizeLeftRight,
                Some(_) => CursorStyle::OpenHand,
                None => CursorStyle::PointingHand,
            };
        }
        if local_y < RULER_HEIGHT {
            return CursorStyle::PointingHand;
        }
        if self.in_footer(local_y) {
            return CursorStyle::Arrow;
        }
        match self.clip_at(&rows(&self.state), local_x, local_y) {
            Some((_, _, ClipPart::TrimStart | ClipPart::TrimEnd)) => CursorStyle::ResizeLeftRight,
            Some((_, _, ClipPart::Header)) => CursorStyle::OpenHand,
            _ => CursorStyle::IBeam,
        }
    }

    pub fn timeline_view(
        &mut self,
        window: &mut Window,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let scene = self.scene(self.rows_in_view(window));
        let waves = self.waves.clone();
        let slot = self.timeline.bounds.clone();
        let span = self.across_span();
        let this = context.weak_entity();
        // Along the bottom of the lanes, over the last of them.
        let across_bar = scrollbar(
            Axis::Horizontal,
            &self.timeline.scroll.across_bar,
            &self.theme,
            move |_| Some(span),
            move |offset, _, app| {
                let _ = this.update(app, |this, context| {
                    this.set_across(offset);
                    context.notify();
                });
            },
        )
        .absolute()
        .left_0()
        .right_0()
        .top(px(self.lanes_bottom() - SCROLLBAR_WIDTH))
        .h(px(SCROLLBAR_WIDTH));

        let view = div()
            .id("timeline")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .cursor(self.cursor())
            .on_mouse_down(MouseButton::Left, context.listener(Self::on_timeline_down))
            .on_mouse_down(MouseButton::Right, context.listener(Self::on_timeline_menu))
            .on_scroll(context.listener(Self::on_timeline_scroll));
        self.take_drops(view, context)
            .child(
                canvas(
                    move |bounds, _, _| slot.set(bounds),
                    move |bounds, _, window, context| {
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            crate::profile::time("paint.timeline", || {
                                paint::paint(
                                    &scene,
                                    bounds,
                                    window,
                                    context,
                                    &mut waves.borrow_mut(),
                                )
                            })
                        })
                    },
                )
                .size_full(),
            )
            .child(across_bar)
    }

    /// Down the track list's far side, along the lanes: the rows scroll
    /// with them.
    pub fn rows_scrollbar(&self, context: &mut Context<Self>) -> impl IntoElement {
        let span = self.rows_span();
        let this = context.weak_entity();
        scrollbar(
            Axis::Vertical,
            &self.timeline.scroll.rows_bar,
            &self.theme,
            move |_| Some(span),
            move |offset, _, app| {
                let _ = this.update(app, |this, context| {
                    this.set_rows(offset);
                    context.notify();
                });
            },
        )
        .absolute()
        .right_0()
        .top(px(RULER_HEIGHT))
        .h(px(span.view.max(0.)))
        .w(px(SCROLLBAR_WIDTH))
    }
}
