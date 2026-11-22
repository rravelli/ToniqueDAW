//! The audio graph, shown in place of the timeline and the track list: the
//! engine's nodes laid out left to right, their connections, output levels
//! and processing time, live, as the egui app's graph view. Dragging pans,
//! the wheel pans, Ctrl+wheel zooms around the pointer, a double click
//! fits.
//!
//! Each frame, `graph_view` takes what's shown into a [`paint::GraphScene`];
//! the canvas paints it.

mod paint;

use std::{cell::Cell, rc::Rc, sync::Arc, time::Duration};

use gpui::{
    Bounds, ContentMask, Context, CursorStyle, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels,
    ScrollEvent, Task, Window, canvas, div, prelude::*,
};
use tonique_engine::graph::GraphTopology;

use crate::{
    app::Tonique,
    config::keymap::Action,
    core::graph_layout::{
        NODE_HEIGHT, NODE_WIDTH, bounds, layout, node_title, stats_line, track_names,
    },
    geometry::Rect,
    smooth_scroll::WheelDelta,
};
use paint::GraphScene;

/// Around the graph when it's fitted.
const FIT_MARGIN: f32 = 40.;
const MIN_ZOOM: f32 = 0.2;
/// Fitting never zooms out further than this: labels stay readable and the
/// rest of the graph is a pan away.
const MIN_FIT_ZOOM: f32 = 0.55;
const MAX_ZOOM: f32 = 3.;
/// Wheel distance (pixels) that zooms by a factor of e.
const ZOOM_WHEEL: f32 = 200.;
/// How often the levels and loads are read again while shown: they move
/// with the transport stopped too (previews).
const REFRESH: Duration = Duration::from_millis(33);

/// Node positions (graph units), by node index.
pub type Positions = Rc<Vec<(f32, f32)>>;

pub struct GraphUi {
    open: bool,
    /// Node positions (graph units) for the topology they were computed for.
    layout: Option<(Arc<GraphTopology>, Positions)>,
    /// Graph units added to node positions before zooming.
    pan: (f32, f32),
    zoom: f32,
    /// Refit whenever the graph changes, until the user pans or zooms.
    auto_fit: bool,
    /// Fit once the view's size is known.
    fit_pending: bool,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// Where the pointer was last while dragging, in the window.
    drag: Option<(f32, f32)>,
    /// Redraws the view while it's shown.
    _ticker: Option<Task<()>>,
}

impl GraphUi {
    pub fn new() -> Self {
        Self {
            open: false,
            layout: None,
            pan: (0., 0.),
            zoom: 1.,
            auto_fit: true,
            fit_pending: false,
            bounds: Rc::default(),
            drag: None,
            _ticker: None,
        }
    }

    /// Whether the graph shows instead of the timeline.
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Positions for `topology`, recomputed when the graph changed, and
    /// whether they were just computed.
    fn positions(&mut self, topology: &Arc<GraphTopology>) -> (Positions, bool) {
        if let Some((laid_out, positions)) = &self.layout
            && Arc::ptr_eq(laid_out, topology)
        {
            return (positions.clone(), false);
        }
        let positions = Rc::new(layout(topology));
        self.layout = Some((topology.clone(), positions.clone()));
        (positions, true)
    }

    /// Zoom and pan so the whole graph is visible in `(width, height)`,
    /// or, if that would make it unreadable, show it from the sources
    /// (left) at a readable size.
    fn fit(&mut self, (width, height): (f32, f32), positions: &[(f32, f32)]) {
        let Some((left, top, right, bottom)) = bounds(positions) else {
            return;
        };
        let (room_x, room_y) = (width - 2. * FIT_MARGIN, height - 2. * FIT_MARGIN);
        let (graph_width, graph_height) = (right - left, bottom - top);
        self.zoom = (room_x / graph_width)
            .min(room_y / graph_height)
            .clamp(MIN_FIT_ZOOM, 1.);
        self.pan = (
            width / 2. / self.zoom - (left + right) / 2.,
            height / 2. / self.zoom - (top + bottom) / 2.,
        );
        if graph_width * self.zoom > room_x {
            self.pan.0 = FIT_MARGIN / self.zoom - left;
        }
    }

    fn view_size(&self) -> (f32, f32) {
        let bounds = Rect::of(self.bounds.get());
        (bounds.width, bounds.height)
    }

    /// The pointer at `(x, y)` in the window, local to the view.
    fn local(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let bounds = Rect::of(self.bounds.get());
        (x - bounds.left, y - bounds.top)
    }

    /// The node under `(x, y)`, local to the view.
    fn node_at(&self, positions: &[(f32, f32)], (x, y): (f32, f32)) -> Option<usize> {
        positions.iter().position(|&position| {
            let (left, top) = self.to_view(position);
            Rect::new(left, top, NODE_WIDTH * self.zoom, NODE_HEIGHT * self.zoom).contains(x, y)
        })
    }

    /// A point in graph units, local to the view.
    fn to_view(&self, (x, y): (f32, f32)) -> (f32, f32) {
        ((x + self.pan.0) * self.zoom, (y + self.pan.1) * self.zoom)
    }

    /// Zooms by `factor`, keeping the point under `anchor` (local) fixed.
    fn zoom_by(&mut self, factor: f32, (anchor_x, anchor_y): (f32, f32)) {
        let graph_x = anchor_x / self.zoom - self.pan.0;
        let graph_y = anchor_y / self.zoom - self.pan.1;
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan = (
            anchor_x / self.zoom - graph_x,
            anchor_y / self.zoom - graph_y,
        );
        self.auto_fit = false;
    }
}

impl Tonique {
    /// Between the timeline and the graph, in builds that have it (debug
    /// ones). The engine measures its nodes only while the graph shows.
    pub fn toggle_graph(&mut self, context: &mut Context<Self>) {
        let graph = &mut self.graph;
        graph.open = !graph.open && Action::ToggleGraphView.is_available();
        graph.drag = None;
        graph._ticker = graph.open.then(|| {
            context.spawn(async move |this, context| {
                loop {
                    context.background_executor().timer(REFRESH).await;
                    if this.update(context, |_, context| context.notify()).is_err() {
                        break;
                    }
                }
            })
        });
        if graph.open {
            graph.fit_pending |= graph.auto_fit;
        }
    }

    pub fn graph_view(
        &mut self,
        window: &mut Window,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let scene = self.graph_scene(window);
        let slot = self.graph.bounds.clone();
        let cursor = if self.graph.is_dragging() {
            CursorStyle::ClosedHand
        } else {
            CursorStyle::Arrow
        };
        div()
            .id("graph")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, context.listener(Self::on_graph_down))
            .on_scroll(context.listener(Self::on_graph_scroll))
            .child(
                canvas(
                    move |bounds, _, _| slot.set(bounds),
                    move |bounds, _, window, context| {
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            crate::profile::time("paint.graph", || {
                                paint::paint(&scene, Rect::of(bounds), window, context)
                            })
                        })
                    },
                )
                .size_full(),
            )
    }

    /// What the canvas paints this frame, fitting the graph first when it
    /// changed or was just shown.
    fn graph_scene(&mut self, window: &Window) -> GraphScene {
        let theme = self.theme.clone();
        let Some(topology) = self.state.graph.topology().cloned() else {
            return GraphScene::empty(theme);
        };
        let graph = &mut self.graph;
        let (positions, relaid) = graph.positions(&topology);
        graph.fit_pending |= relaid && graph.auto_fit;
        let size = graph.view_size();
        // The size is known from the second frame on.
        if graph.fit_pending && size.0 > 0. && size.1 > 0. {
            graph.fit(size, &positions);
            graph.fit_pending = false;
        }

        let pointer = window.mouse_position();
        let pointer = graph.local((f32::from(pointer.x), f32::from(pointer.y)));
        let inside = (0. ..size.0).contains(&pointer.0) && (0. ..size.1).contains(&pointer.1);
        let hovered = (inside && self.popup.is_none())
            .then(|| graph.node_at(&positions, pointer))
            .flatten();

        let monitor = &self.state.graph;
        let owners = track_names(&self.state);
        let titles = topology
            .nodes
            .iter()
            .map(|node| node_title(node, &owners))
            .collect();
        let nodes_cpu: f32 = monitor.cpu.iter().sum();
        GraphScene {
            theme,
            stats: stats_line(&topology, self.state.metrics.latency, nodes_cpu),
            topology: Some(topology),
            positions,
            pan: graph.pan,
            zoom: graph.zoom,
            titles,
            levels: monitor.levels.clone(),
            cpu: monitor.cpu.clone(),
            hovered: hovered.map(|node| (node, pointer)),
        }
    }

    fn on_graph_down(
        &mut self,
        event: &MouseDownEvent,
        _: &mut Window,
        context: &mut Context<Self>,
    ) {
        let graph = &mut self.graph;
        if event.click_count == 2 {
            if let Some((_, positions)) = &graph.layout {
                let positions = positions.clone();
                graph.fit(graph.view_size(), &positions);
            }
            graph.auto_fit = true;
        } else {
            graph.drag = Some((f32::from(event.position.x), f32::from(event.position.y)));
        }
        context.notify();
    }

    /// The wheel pans (across with Shift held); with Ctrl held, it zooms
    /// around the pointer.
    fn on_graph_scroll(
        &mut self,
        event: &ScrollEvent,
        _: &mut Window,
        context: &mut Context<Self>,
    ) {
        let wheel = WheelDelta::of(event.delta);
        let graph = &mut self.graph;
        if event.modifiers.control || event.modifiers.platform {
            let anchor = graph.local((f32::from(event.position.x), f32::from(event.position.y)));
            graph.zoom_by((wheel.vertical / ZOOM_WHEEL).exp(), anchor);
        } else {
            let (across, down) = if event.modifiers.shift {
                (wheel.vertical, 0.)
            } else {
                (wheel.horizontal, wheel.vertical)
            };
            graph.pan.0 += across / graph.zoom;
            graph.pan.1 += down / graph.zoom;
            graph.auto_fit = false;
        }
        context.notify();
    }

    /// Follows a pan by dragging. Returns whether one is going on.
    pub(crate) fn graph_drag_move(&mut self, event: &MouseMoveEvent) -> bool {
        let graph = &mut self.graph;
        let Some((last_x, last_y)) = graph.drag else {
            return false;
        };
        let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
        graph.pan.0 += (x - last_x) / graph.zoom;
        graph.pan.1 += (y - last_y) / graph.zoom;
        graph.drag = Some((x, y));
        graph.auto_fit = false;
        true
    }

    pub(crate) fn graph_drag_end(&mut self) {
        self.graph.drag = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_shows_every_node() {
        let positions = [(0., -72.), (0., 72.), (240., 0.), (480., 0.)];
        let mut graph = GraphUi::new();
        let size = (1200., 800.);
        graph.fit(size, &positions);
        for &position in &positions {
            let (left, top) = graph.to_view(position);
            let (right, bottom) =
                graph.to_view((position.0 + NODE_WIDTH, position.1 + NODE_HEIGHT));
            assert!(left >= 0. && top >= 0., "{position:?} off the top left");
            assert!(
                right <= size.0 && bottom <= size.1,
                "{position:?} off the bottom right"
            );
        }
        assert_eq!(
            graph.node_at(&positions, graph.to_view((250., 10.))),
            Some(2)
        );
        assert_eq!(graph.node_at(&positions, graph.to_view((-50., 0.))), None);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let mut graph = GraphUi::new();
        graph.pan = (30., -20.);
        let anchor = (400., 300.);
        let before = (
            anchor.0 / graph.zoom - graph.pan.0,
            anchor.1 / graph.zoom - graph.pan.1,
        );
        graph.zoom_by(1.5, anchor);
        let after = graph.to_view(before);
        assert!((after.0 - anchor.0).abs() < 1e-3 && (after.1 - anchor.1).abs() < 1e-3);
        graph.zoom_by(100., anchor);
        assert_eq!(graph.zoom, MAX_ZOOM);
    }
}
