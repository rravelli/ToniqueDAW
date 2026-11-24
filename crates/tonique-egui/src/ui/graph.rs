use crate::ui::theme::ToEgui;
use crate::ui::theme::font_size;
use crate::ui::theme::{Theme, ThemeExt};
use egui::{
    Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind,
    Ui, Vec2, epaint::CubicBezierShape, pos2, vec2,
};
use std::{sync::Arc, time::Duration};
use tonique_core::{
    diagnostics::graph_layout::{
        self, NODE_HEIGHT, NODE_WIDTH, elide, meter_unit, node_details, node_palette_slot,
        node_title, stats_line, track_names,
    },
    diagnostics::graph_monitor::GraphMonitor,
    state::ProjectState,
};
use tonique_engine::graph::{GraphTopology, TopologyNode};

// Sizes in graph units (scaled by the zoom).
const NODE_SIZE: Vec2 = vec2(NODE_WIDTH, NODE_HEIGHT);
const FIT_MARGIN: f32 = 40.;
const MIN_ZOOM: f32 = 0.2;
/// Fitting never zooms out further than this: labels stay readable and the
/// rest of the graph is a pan away.
const MIN_FIT_ZOOM: f32 = 0.55;
const MAX_ZOOM: f32 = 3.;
/// Below this zoom, node text is unreadable: skip it.
const TEXT_ZOOM: f32 = 0.45;

/// Live view of the engine's processing graph: every scheduled node, its
/// connections, output level and processing time.
pub struct GraphView {
    /// Node positions (graph units) for the topology they were computed for.
    layout: Option<(Arc<GraphTopology>, Vec<Pos2>)>,
    pan: Vec2,
    zoom: f32,
    /// Refit whenever the graph changes, until the user pans or zooms.
    auto_fit: bool,
}

impl GraphView {
    pub fn new() -> Self {
        Self {
            layout: None,
            pan: Vec2::ZERO,
            zoom: 1.,
            auto_fit: true,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &mut ProjectState) {
        let (viewport, response) =
            ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let theme = ui.app_theme();
        let painter = ui.painter_at(viewport);
        painter.rect_filled(viewport, 0., theme.bg_deep);
        // Levels move even with the transport stopped (e.g. previews).
        ui.ctx().request_repaint_after(Duration::from_millis(33));

        let monitor = &state.graph();
        let Some(topology) = monitor.topology().cloned() else {
            painter.text(
                viewport.center(),
                Align2::CENTER_CENTER,
                "No graph yet",
                FontId::proportional(font_size::HEADING),
                theme.text_muted,
            );
            return;
        };
        let (positions, relaid) = self.positions(&topology);
        if relaid && self.auto_fit {
            self.fit(viewport, &positions);
        }

        self.handle_input(ui, &response, viewport, &positions);
        let to_screen = |p: Pos2| viewport.min + (p.to_vec2() + self.pan) * self.zoom;

        // Edges under nodes.
        for (i, node) in topology.nodes.iter().enumerate() {
            let to = to_screen(positions[i]) + vec2(0., NODE_SIZE.y / 2. * self.zoom);
            for &from in &node.inputs {
                let start = to_screen(positions[from])
                    + vec2(NODE_SIZE.x * self.zoom, NODE_SIZE.y / 2. * self.zoom);
                let level = meter_unit(monitor.levels.get(from).copied().unwrap_or(0.));
                let color = theme.text_disabled.lerp_to_gamma(theme.accent, level);
                painter.add(edge(start, to, Stroke::new(1. + 2. * level, color)));
            }
            for &from in &node.after {
                let start = to_screen(positions[from])
                    + vec2(NODE_SIZE.x * self.zoom, NODE_SIZE.y / 2. * self.zoom);
                let curve = edge(start, to, Stroke::NONE);
                painter.extend(Shape::dashed_line(
                    &curve.flatten(Some(0.5)),
                    Stroke::new(1., theme.text_disabled),
                    4.,
                    4.,
                ));
            }
        }

        let owners = track_names(state);
        let titles: Vec<(String, Option<Color32>)> = topology
            .nodes
            .iter()
            .map(|node| {
                let (title, color) = node_title(node, &owners);
                (title, color.map(|color| color.to_egui()))
            })
            .collect();
        let hovered = response.hover_pos().and_then(|p| {
            (0..positions.len()).find(|&i| self.node_rect(positions[i], &to_screen).contains(p))
        });
        for (i, node) in topology.nodes.iter().enumerate() {
            let rect = self.node_rect(positions[i], &to_screen);
            if !viewport.intersects(rect) {
                continue;
            }
            let (title, stripe) = &titles[i];
            self.paint_node(
                &painter,
                &theme,
                rect,
                node,
                title,
                *stripe,
                i == topology.output,
                hovered == Some(i),
                monitor,
                i,
            );
        }

        paint_header(
            &painter,
            &theme,
            viewport,
            &topology,
            state.metrics().latency,
            monitor,
        );

        if let Some(i) = hovered {
            response.on_hover_ui_at_pointer(|ui| {
                node_tooltip(ui, &topology.nodes[i], &titles[i].0, monitor, i)
            });
        }
    }

    /// Positions for `topology` (recomputed when the graph changed), and
    /// whether they were just computed.
    fn positions(&mut self, topology: &Arc<GraphTopology>) -> (Vec<Pos2>, bool) {
        match &self.layout {
            Some((t, positions)) if Arc::ptr_eq(t, topology) => (positions.clone(), false),
            _ => {
                let positions = layout(topology);
                self.layout = Some((topology.clone(), positions.clone()));
                (positions, true)
            }
        }
    }

    fn node_rect(&self, pos: Pos2, to_screen: &impl Fn(Pos2) -> Pos2) -> Rect {
        Rect::from_min_size(to_screen(pos), NODE_SIZE * self.zoom)
    }

    fn handle_input(&mut self, ui: &Ui, response: &Response, viewport: Rect, positions: &[Pos2]) {
        if response.double_clicked() {
            self.fit(viewport, positions);
            self.auto_fit = true;
        }
        if response.dragged() {
            self.pan += response.drag_delta() / self.zoom;
            self.auto_fit = false;
        }
        if !response.hovered() {
            return;
        }
        let (zoom_delta, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
        if zoom_delta != 1.
            && let Some(mouse) = response.hover_pos()
        {
            // Keep the point under the mouse fixed.
            let anchor = (mouse - viewport.min) / self.zoom - self.pan;
            self.zoom = (self.zoom * zoom_delta).clamp(MIN_ZOOM, MAX_ZOOM);
            self.pan = (mouse - viewport.min) / self.zoom - anchor;
            self.auto_fit = false;
        }
        if scroll != Vec2::ZERO {
            self.pan += scroll / self.zoom;
            self.auto_fit = false;
        }
    }

    /// Zoom and pan so the whole graph is visible, or, if that would make it
    /// unreadable, show it from the sources (left) at a readable size.
    fn fit(&mut self, viewport: Rect, positions: &[Pos2]) {
        let Some(bounds) = positions
            .iter()
            .map(|p| Rect::from_min_size(*p, NODE_SIZE))
            .reduce(|a, b| a.union(b))
        else {
            return;
        };
        let avail = viewport.size() - Vec2::splat(2. * FIT_MARGIN);
        self.zoom = (avail.x / bounds.width())
            .min(avail.y / bounds.height())
            .clamp(MIN_FIT_ZOOM, 1.);
        let center = viewport.size() / 2. / self.zoom;
        self.pan = center - bounds.center().to_vec2();
        if bounds.width() * self.zoom > avail.x {
            self.pan.x = FIT_MARGIN / self.zoom - bounds.left();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_node(
        &self,
        painter: &egui::Painter,
        theme: &Theme,
        rect: Rect,
        node: &TopologyNode,
        title: &str,
        stripe: Option<Color32>,
        is_output: bool,
        hovered: bool,
        monitor: &GraphMonitor,
        index: usize,
    ) {
        let z = self.zoom;
        let fill = node_color(node.name, theme);
        let stroke = if is_output {
            Stroke::new(2., theme.accent)
        } else if hovered {
            Stroke::new(1.5, theme.text)
        } else {
            Stroke::new(1., theme.bg_deep)
        };
        painter.rect(
            rect,
            CornerRadius::same((6. * z) as u8),
            fill,
            stroke,
            StrokeKind::Inside,
        );
        // Track color along the left edge.
        if let Some(color) = stripe {
            let stripe = Rect::from_min_max(
                rect.left_top() + vec2(2., 5.) * z,
                rect.left_bottom() + vec2(5., -5.) * z,
            );
            painter.rect_filled(stripe, 1., color);
        }

        // Level bar along the bottom.
        let level = monitor.levels.get(index).copied().unwrap_or(0.);
        let bar = Rect::from_min_max(
            pos2(rect.left() + 6. * z, rect.bottom() - 9. * z),
            pos2(rect.right() - 6. * z, rect.bottom() - 5. * z),
        );
        painter.rect_filled(bar, 1., theme.bg_deep);
        if node.channels > 0 {
            let mut lit = bar;
            lit.set_width(bar.width() * meter_unit(level));
            let color = if level > 1. {
                theme.meter_high
            } else {
                theme.accent
            };
            painter.rect_filled(lit, 1., color);
        }

        if z < TEXT_ZOOM {
            return;
        }
        let text = theme.text;
        painter.text(
            rect.left_top() + vec2(8., 6.) * z,
            Align2::LEFT_TOP,
            elide(title, 26),
            FontId::proportional(font_size::LABEL * z),
            text,
        );
        if node.label.is_some() {
            painter.text(
                rect.left_top() + vec2(8., 21.) * z,
                Align2::LEFT_TOP,
                node.name,
                FontId::proportional(font_size::SMALL * z),
                theme.text_muted,
            );
        }
        let cpu = monitor.cpu.get(index).copied().unwrap_or(0.);
        painter.text(
            rect.right_top() + vec2(-8., 21.) * z,
            Align2::RIGHT_TOP,
            format!("{:.1}%", cpu * 100.),
            FontId::monospace(font_size::SMALL * z),
            theme.text_muted,
        );
    }
}

fn edge(from: Pos2, to: Pos2, stroke: Stroke) -> CubicBezierShape {
    let bend = ((to.x - from.x).abs() / 2.).max(30.);
    CubicBezierShape::from_points_stroke(
        [from, from + vec2(bend, 0.), to - vec2(bend, 0.), to],
        false,
        Color32::TRANSPARENT,
        stroke,
    )
}

fn paint_header(
    painter: &egui::Painter,
    theme: &Theme,
    viewport: Rect,
    topology: &GraphTopology,
    engine_load: f32,
    monitor: &GraphMonitor,
) {
    let nodes_cpu: f32 = monitor.cpu.iter().sum();
    let text = stats_line(topology, engine_load, nodes_cpu);
    let pos = viewport.left_top() + vec2(10., 8.);
    let galley = painter.layout_no_wrap(
        text,
        FontId::proportional(font_size::LABEL),
        theme.text_muted,
    );
    painter.rect_filled(
        Rect::from_min_size(pos, galley.size()).expand(4.),
        3.,
        theme.shadow,
    );
    painter.galley(pos, galley, theme.text_muted);
    painter.text(
        viewport.left_bottom() + vec2(10., -8.),
        Align2::LEFT_BOTTOM,
        "Drag to pan · Ctrl+scroll to zoom · Double-click to fit",
        FontId::proportional(font_size::SMALL),
        theme.text_disabled,
    );
}

fn node_tooltip(
    ui: &mut Ui,
    node: &TopologyNode,
    title: &str,
    monitor: &GraphMonitor,
    index: usize,
) {
    let level = monitor.levels.get(index).copied().unwrap_or(0.);
    let cpu = monitor.cpu.get(index).copied().unwrap_or(0.);
    for (line, text) in node_details(node, title, level, cpu)
        .into_iter()
        .enumerate()
    {
        if line == 0 && node.label.is_some() {
            ui.strong(text);
        } else {
            ui.label(text);
        }
    }
}

/// Tint by what the node does: a palette colour mixed into the surface, so
/// the text stays readable.
fn node_color(name: &str, theme: &Theme) -> Color32 {
    let Some(slot) = node_palette_slot(name) else {
        return theme.bg_control;
    };
    match theme.palette.len() {
        0 => theme.bg_control,
        len => theme
            .bg_raised
            .lerp_to_gamma(theme.palette[slot % len], 0.35),
    }
}

/// Positions in egui's points.
fn layout(topology: &GraphTopology) -> Vec<Pos2> {
    graph_layout::layout(topology)
        .into_iter()
        .map(|(x, y)| pos2(x, y))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::ToColor;
    use tonique_engine::engine::{Engine, EngineConfig};

    #[test]
    fn draws_the_live_graph_headless() {
        use crate::ui::theme::Theme;
        use tonique_core::state::ProjectState;

        let (engine, _p) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        let palette: Vec<tonique_core::utils::color::Color> = Theme::dark()
            .palette
            .iter()
            .map(|&c| c.to_color())
            .collect();
        state.set_track_palette(&palette);
        state.add_track();
        state.add_track();
        state.set_monitor_graph(true);
        state.update(); // enables metering, picks up the topology
        let topology = state.graph().topology().cloned().expect("graph published");
        assert!(topology.meters().is_enabled());

        let mut view = GraphView::new();
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1200., 800.));
        let input = || egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let mut shapes = 0;
        for _ in 0..2 {
            let mut output = ctx.run_ui(input(), |ui| view.ui(ui, &mut state));
            output.textures_delta.clear(); // no GPU here
            shapes = output.shapes.len();
        }
        assert!(shapes > topology.nodes.len(), "only {shapes} shapes");

        // Fitted: every node is on screen.
        let (positions, _) = view.positions(&topology);
        let to_screen = |p: Pos2| screen.min + (p.to_vec2() + view.pan) * view.zoom;
        for p in &positions {
            assert!(
                screen.contains_rect(view.node_rect(*p, &to_screen)),
                "node off screen at {p:?}"
            );
        }

        // Leaving the view stops measuring.
        state.set_monitor_graph(false);
        state.update();
        assert!(!topology.meters().is_enabled());
    }
}
