//! The arrangement: the ruler (loop lane, bars, time), a lane per track with
//! its clips, the grid, the playhead and the edit cursor. Edits go through
//! Tonique's `ProjectState` and `GridService`, as in the egui app.
//!
//! The [`Arrangement`] is a view of its own: the timeline and, on its
//! right, the track list (`crate::headers`), one per row and scrolled
//! together, so one view: they share the rows, the scroll, the hover and
//! drags going from one to the other.
//!
//! Each frame, `timeline_view` takes what's shown out of the state into a
//! [`scene::Scene`]; the canvas paints it ([`paint`]). Pointer handling is
//! on the wrapping div ([`input`], [`drop`]); drags are then followed over
//! the whole window ([`follow_drag`]).

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

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

use gpui::{
    Anchor, App, Bounds, ContentMask, Context, CursorStyle, Entity, EventEmitter, Modifiers,
    MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, Point, Window, canvas, div, prelude::*, px,
};

pub use crate::core::row_layout::{ROW_GAP, Row, RowLayout};
pub use drop::DropTarget;
pub use rows::{FOOTER_PADDING, footer_height};

use tonique_engine::edit::{ClipId, TrackId};

use crate::{
    app::PopupKind,
    headers::HeadersUi,
    model::{HoldsProject, Project},
    popup::{self, MenuBuilder},
    theme::hsla,
    wave::WaveCache,
    widgets::{Axis, SCROLLBAR_WIDTH, follow_drag, scrollbar},
};
use input::TimelineDrag;
use rows::ClipPart;
use ruler::LoopGrab;

pub const LOOP_HEIGHT: f32 = 12.;
pub const BARS_HEIGHT: f32 = 20.;
pub const TIME_HEIGHT: f32 = 16.;
pub const RULER_HEIGHT: f32 = LOOP_HEIGHT + BARS_HEIGHT + TIME_HEIGHT;
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

/// The timeline and the track list beside it.
pub struct Arrangement {
    project: Entity<Project>,
    /// The waveforms drawn, shared with the other views.
    waves: Rc<RefCell<WaveCache>>,
    pub timeline: TimelineUi,
    pub headers: HeadersUi,
    /// The window's width, as last drawn: the list leaves the timeline half
    /// of it.
    window_width: f32,
}

/// What the arrangement asks of the window.
pub enum ArrangementEvent {
    /// The bottom panel on the selected track's effects.
    ShowEffects,
    /// The bottom panel on clip `id`'s details.
    ShowClipDetails(ClipId),
    /// As a double-click on a track header: the bottom panel on its
    /// effects, or closed when it showed them; whether it was selected.
    ToggleEffectsOfSelected(bool),
    /// The browser's missing files, a clip's file selected.
    ShowMissingFiles(Option<PathBuf>),
}

impl EventEmitter<ArrangementEvent> for Arrangement {}

impl HoldsProject for Arrangement {
    fn project(&self) -> &Entity<Project> {
        &self.project
    }
}

impl Arrangement {
    /// What its waveforms take.
    pub fn wave_memory(&self) -> crate::wave::WaveMemory {
        self.waves.borrow().memory()
    }

    pub fn new(project: Entity<Project>) -> Self {
        Self {
            project,
            waves: Rc::default(),
            timeline: TimelineUi::default(),
            headers: HeadersUi::new(),
            window_width: 0.,
        }
    }

    /// What referred to a replaced project's tracks and clips.
    pub fn forget_project(&mut self) {
        self.headers.forget_project();
        self.timeline.forget_project();
    }

    fn is_dragging(&self) -> bool {
        self.timeline.is_dragging() || self.headers.is_dragging()
    }

    /// Follows a drag in the timeline or the track list.
    fn drag_move(&mut self, event: &MouseMoveEvent, context: &mut Context<Self>) {
        let width = self.window_width;
        self.timeline_drag_move(event, context);
        self.header_drag_move(event, width, context);
    }

    fn drag_end(&mut self, event: &MouseUpEvent, context: &mut Context<Self>) {
        self.timeline_drag_end(event, context);
        self.header_drag_end(context);
    }

    /// Builds one of the arrangement's menus each frame.
    pub fn menu_builder(&self, kind: PopupKind, context: &Context<Self>) -> MenuBuilder {
        popup::builder(context.weak_entity(), move |this, _, context| match kind {
            PopupKind::Grid => Some(this.grid_menu(context)),
            PopupKind::Clip(id) => this.clip_menu(id, context),
            PopupKind::Track(id) => this.track_menu(id, context),
            _ => None,
        })
    }

    /// Builds the grid's menu each frame, for the transport bar's button.
    pub fn grid_menu_builder(&self, this: &Entity<Self>) -> MenuBuilder {
        popup::builder(this.downgrade(), |this, _, context| {
            Some(this.grid_menu(context))
        })
    }

    /// Opens one of the arrangement's menus with its `anchor` corner at
    /// `at`; or, when it's open, closes it.
    pub(crate) fn toggle_popup(
        &mut self,
        kind: PopupKind,
        at: Point<Pixels>,
        anchor: Anchor,
        context: &mut Context<Self>,
    ) {
        let build = self.menu_builder(kind, context);
        popup::toggle(kind, at, anchor, build, context);
    }

    fn cursor(&self, rows: &RowLayout, context: &App) -> CursorStyle {
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
        if local_y < LOOP_HEIGHT && !self.on_playhead_head(local_x, local_y, context) {
            return match self.loop_grab_at(local_x, context) {
                Some(LoopGrab::Start | LoopGrab::End) => CursorStyle::ResizeLeftRight,
                Some(_) => CursorStyle::OpenHand,
                None => CursorStyle::PointingHand,
            };
        }
        if local_y < RULER_HEIGHT {
            return CursorStyle::PointingHand;
        }
        if self.in_footer(local_y, context) {
            return CursorStyle::Arrow;
        }
        match self.clip_at(rows, local_x, local_y, context) {
            Some((_, _, ClipPart::TrimStart | ClipPart::TrimEnd)) => CursorStyle::ResizeLeftRight,
            Some((_, _, ClipPart::Header)) => CursorStyle::OpenHand,
            _ => CursorStyle::IBeam,
        }
    }

    /// The ruler and the lanes of `rows`, the rows this frame shows.
    pub fn timeline_view(
        &mut self,
        rows: &RowLayout,
        window: &mut Window,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let in_view = self.rows_in_view(window, context);
        let scene = self.scene(rows, in_view, crate::theme::current(context), context);
        let waves = self.waves.clone();
        let slot = self.timeline.bounds.clone();
        let span = self.across_span(context);
        let this = context.weak_entity();
        // Along the bottom of the lanes, over the last of them.
        let across_bar = scrollbar(
            Axis::Horizontal,
            &self.timeline.scroll.across_bar,
            &crate::theme::current(context),
            move |_| Some(span),
            move |offset, _, app| {
                let _ = this.update(app, |this, context| {
                    this.set_across(offset, context);
                    context.notify();
                });
            },
        )
        .absolute()
        .left_0()
        .right_0()
        .top(px(self.lanes_bottom(context) - SCROLLBAR_WIDTH))
        .h(px(SCROLLBAR_WIDTH));

        let view = div()
            .id("timeline")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .cursor(self.cursor(rows, context))
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
    pub fn rows_scrollbar(
        &self,
        rows: &RowLayout,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let span = self.rows_span(rows, context);
        let this = context.weak_entity();
        scrollbar(
            Axis::Vertical,
            &self.timeline.scroll.rows_bar,
            &crate::theme::current(context),
            move |_| Some(span),
            move |offset, _, app| {
                let _ = this.update(app, |this, context| {
                    this.set_rows(offset, context);
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

impl Render for Arrangement {
    fn render(&mut self, window: &mut Window, context: &mut Context<Self>) -> impl IntoElement {
        self.waves.borrow_mut().start_frame(window);
        // The playhead, the meters in the track list.
        self.watch_playback(context);
        self.window_width = f32::from(window.viewport_size().width);
        let scrolling = self.step_scrolling(context) | self.autoscroll(context);
        let dragging = context.has_active_drag() || self.is_dragging();
        self.follow_playhead(dragging, context);
        if scrolling || dragging {
            window.request_animation_frame();
        }
        if !context.has_active_drag() {
            self.timeline.drop_target = None;
            self.timeline.effect_target = None;
        }
        // The rows as this frame shows them, for both: laid out once each
        // time the project changes.
        let rows = self.rows(context);
        let theme = crate::theme::current(context);
        let following = self.is_dragging();
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .h_full()
            // Redraws as the pointer moves: the cursor and the lit parts
            // follow it.
            .on_mouse_move(
                context.listener(|this, event: &MouseMoveEvent, _, context| {
                    if this.timeline_hover(event) {
                        context.notify();
                    }
                }),
            )
            .child(crate::profile::time("view.timeline", || {
                self.timeline_view(&rows, window, context)
            }))
            .child(
                div()
                    .flex_none()
                    .w(px(1.))
                    .h_full()
                    .bg(hsla(theme.separator)),
            )
            .child(crate::profile::time("view.headers", || {
                self.headers_view(&rows, window, context)
            }))
            // A drag follows the pointer anywhere in the window.
            .when(following, |element| {
                element.child(follow_drag(
                    context.weak_entity(),
                    Self::drag_move,
                    Self::drag_end,
                ))
            })
    }
}
