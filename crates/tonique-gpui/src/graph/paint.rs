//! Painting the graph: connections under the nodes, each node with its
//! track's colour, level bar and load, the statistics on top, and the
//! hovered node's details by the pointer.

use std::{rc::Rc, sync::Arc};

use gpui::{PathBuilder, Window, point, px};
use tonique_engine::graph::{GraphTopology, TopologyNode};

use super::Positions;
use crate::{
    bottom::paint::{Align, fill, fill_rounded, text},
    geometry::Rect,
    theme::{Color, Theme, hsla, mix, mono, size},
    widgets::{paint_line, shape},
};
use tonique_core::monitor::graph_layout::{
    NODE_HEIGHT, NODE_WIDTH, elide, meter_unit, node_details, node_palette_slot,
};

/// Below this zoom, node text is unreadable: skip it.
const TEXT_ZOOM: f32 = 0.45;
/// Characters of a node's title shown.
const TITLE_CHARACTERS: usize = 26;
const HINT: &str = "Drag to pan · Ctrl+scroll to zoom · Double-click to fit";

/// What's shown, taken out of the app for one frame.
pub struct GraphScene {
    pub theme: std::rc::Rc<Theme>,
    /// `None` until the engine publishes its graph.
    pub topology: Option<Arc<GraphTopology>>,
    pub positions: Positions,
    pub pan: (f32, f32),
    pub zoom: f32,
    /// Each node's title and its track's colour.
    pub titles: Vec<(String, Option<Color>)>,
    pub levels: Vec<f32>,
    pub cpu: Vec<f32>,
    /// The node under the pointer, and the pointer, local to the view.
    pub hovered: Option<(usize, (f32, f32))>,
    pub stats: String,
}

impl GraphScene {
    pub fn empty(theme: std::rc::Rc<Theme>) -> Self {
        Self {
            theme,
            topology: None,
            positions: Rc::default(),
            pan: (0., 0.),
            zoom: 1.,
            titles: Vec::new(),
            levels: Vec::new(),
            cpu: Vec::new(),
            hovered: None,
            stats: String::new(),
        }
    }

    /// A node's rectangle in the window, the view at `view`.
    fn node_rect(&self, view: Rect, index: usize) -> Rect {
        let (x, y) = self.positions[index];
        Rect::new(
            view.left + (x + self.pan.0) * self.zoom,
            view.top + (y + self.pan.1) * self.zoom,
            NODE_WIDTH * self.zoom,
            NODE_HEIGHT * self.zoom,
        )
    }

    fn level(&self, index: usize) -> f32 {
        self.levels.get(index).copied().unwrap_or(0.)
    }

    fn cpu(&self, index: usize) -> f32 {
        self.cpu.get(index).copied().unwrap_or(0.)
    }
}

pub fn paint(scene: &GraphScene, view: Rect, window: &mut Window, context: &mut gpui::App) {
    let theme = &scene.theme;
    fill(window, view, theme.bg_deep);
    let Some(topology) = &scene.topology else {
        text(
            window,
            context,
            "No graph yet",
            (view.left + view.width / 2., view.top + view.height / 2.),
            Align::CENTER,
            size::HEADING,
            theme.text_muted,
            None,
        );
        return;
    };

    paint_edges(scene, topology, view, window);
    for (index, node) in topology.nodes.iter().enumerate() {
        let rect = scene.node_rect(view, index);
        if !rect.intersect(view).is_empty() {
            paint_node(
                scene,
                node,
                index,
                index == topology.output,
                rect,
                window,
                context,
            );
        }
    }

    // The statistics on a shadow, the hint along the bottom.
    let line = shape(
        window,
        scene.stats.clone(),
        size::LABEL,
        hsla(theme.text_muted),
        gpui::font(crate::theme::FONT_UI),
    );
    let (left, top) = (view.left + 10., view.top + 8.);
    fill_rounded(
        window,
        Rect::new(
            left - 4.,
            top - 4.,
            f32::from(line.width) + 8.,
            size::LABEL * 1.25 + 8.,
        ),
        3.,
        theme.shadow,
    );
    paint_line(window, context, &line, left, top, size::LABEL);
    text(
        window,
        context,
        HINT,
        (view.left + 10., view.bottom() - 8.),
        Align::LEFT_BOTTOM,
        size::SMALL,
        theme.text_disabled,
        None,
    );

    if let Some((index, pointer)) = scene.hovered {
        let (title, _) = &scene.titles[index];
        let lines = node_details(
            &topology.nodes[index],
            title,
            scene.level(index),
            scene.cpu(index),
        );
        paint_details(scene, &lines, view, pointer, window, context);
    }
}

/// Audio connections lit by the level going through them; ordering-only
/// ones dashed.
fn paint_edges(scene: &GraphScene, topology: &GraphTopology, view: Rect, window: &mut Window) {
    let theme = &scene.theme;
    let input_at = |index: usize| {
        let rect = scene.node_rect(view, index);
        (rect.left, rect.top + rect.height / 2.)
    };
    let output_at = |index: usize| {
        let rect = scene.node_rect(view, index);
        (rect.right(), rect.top + rect.height / 2.)
    };
    for (index, node) in topology.nodes.iter().enumerate() {
        let to = input_at(index);
        for &from in &node.inputs {
            let level = meter_unit(scene.level(from));
            let color = mix(theme.text_disabled, theme.accent, level);
            edge(window, output_at(from), to, 1. + 2. * level, color, false);
        }
        for &from in &node.after {
            edge(window, output_at(from), to, 1., theme.text_disabled, true);
        }
    }
}

/// A curve leaving `from` rightwards and reaching `to` from the left.
fn edge(
    window: &mut Window,
    (from_x, from_y): (f32, f32),
    (to_x, to_y): (f32, f32),
    width: f32,
    color: Color,
    dashed: bool,
) {
    let bend = ((to_x - from_x).abs() / 2.).max(30.);
    let mut path = PathBuilder::stroke(px(width));
    if dashed {
        path = path.dash_array(&[px(4.), px(4.)]);
    }
    path.move_to(point(px(from_x), px(from_y)));
    path.cubic_bezier_to(
        point(px(to_x), px(to_y)),
        point(px(from_x + bend), px(from_y)),
        point(px(to_x - bend), px(to_y)),
    );
    if let Ok(path) = path.build() {
        window.paint_path(path, hsla(color));
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_node(
    scene: &GraphScene,
    node: &TopologyNode,
    index: usize,
    is_output: bool,
    rect: Rect,
    window: &mut Window,
    context: &mut gpui::App,
) {
    let theme = &scene.theme;
    let zoom = scene.zoom;
    let hovered = scene.hovered.is_some_and(|(hovered, _)| hovered == index);
    let (border, border_color) = if is_output {
        (2., theme.accent)
    } else if hovered {
        (1.5, theme.text)
    } else {
        (1., theme.bg_deep)
    };
    window.paint_quad(
        gpui::fill(rect.bounds(), hsla(node_color(node.name, theme)))
            .corner_radii(px(6. * zoom))
            .border_widths(px(border))
            .border_color(hsla(border_color)),
    );
    // The track's colour along the left edge.
    let (title, stripe) = &scene.titles[index];
    if let Some(color) = stripe {
        let stripe = Rect::new(
            rect.left + 2. * zoom,
            rect.top + 5. * zoom,
            3. * zoom,
            rect.height - 10. * zoom,
        );
        fill_rounded(window, stripe, 1., *color);
    }

    // The level along the bottom.
    let bar = Rect::new(
        rect.left + 6. * zoom,
        rect.bottom() - 9. * zoom,
        rect.width - 12. * zoom,
        4. * zoom,
    );
    fill_rounded(window, bar, 1., theme.bg_deep);
    if node.channels > 0 {
        let level = scene.level(index);
        let lit = Rect {
            width: bar.width * meter_unit(level),
            ..bar
        };
        let color = if level > 1. {
            theme.meter_high
        } else {
            theme.accent
        };
        if !lit.is_empty() {
            fill_rounded(window, lit, 1., color);
        }
    }

    if zoom < TEXT_ZOOM {
        return;
    }
    text(
        window,
        context,
        elide(title, TITLE_CHARACTERS),
        (rect.left + 8. * zoom, rect.top + 6. * zoom),
        Align::LEFT_TOP,
        size::LABEL * zoom,
        theme.text,
        None,
    );
    if node.label.is_some() {
        text(
            window,
            context,
            node.name,
            (rect.left + 8. * zoom, rect.top + 21. * zoom),
            Align::LEFT_TOP,
            size::SMALL * zoom,
            theme.text_muted,
            None,
        );
    }
    text(
        window,
        context,
        format!("{:.1}%", scene.cpu(index) * 100.),
        (rect.right() - 8. * zoom, rect.top + 21. * zoom),
        Align::RIGHT_TOP,
        size::SMALL * zoom,
        theme.text_muted,
        Some(mono()),
    );
}

/// The hovered node's details in a box by the pointer, kept in the view.
fn paint_details(
    scene: &GraphScene,
    lines: &[String],
    view: Rect,
    (pointer_x, pointer_y): (f32, f32),
    window: &mut Window,
    context: &mut gpui::App,
) {
    const PADDING: f32 = 6.;
    let theme = &scene.theme;
    let line_height = size::BODY * 1.25;
    let shaped: Vec<_> = lines
        .iter()
        .map(|line| {
            shape(
                window,
                line.clone(),
                size::BODY,
                hsla(theme.text),
                gpui::font(crate::theme::FONT_UI),
            )
        })
        .collect();
    let widest = shaped
        .iter()
        .map(|line| f32::from(line.width))
        .fold(0., f32::max);
    let (width, height) = (
        widest + 2. * PADDING,
        lines.len() as f32 * line_height + 2. * PADDING,
    );
    let left = (view.left + pointer_x + 14.)
        .min(view.right() - width)
        .max(view.left);
    let top = (view.top + pointer_y + 18.)
        .min(view.bottom() - height)
        .max(view.top);
    let rect = Rect::new(left, top, width, height);
    window.paint_quad(
        gpui::fill(rect.bounds(), hsla(theme.bg_raised))
            .corner_radii(px(4.))
            .border_widths(px(1.))
            .border_color(hsla(theme.separator)),
    );
    for (row, line) in shaped.iter().enumerate() {
        paint_line(
            window,
            context,
            line,
            left + PADDING,
            top + PADDING + row as f32 * line_height,
            size::BODY,
        );
    }
}

/// Tint by what the node does: a palette colour mixed into the surface, so
/// the text stays readable.
fn node_color(name: &str, theme: &Theme) -> Color {
    match (node_palette_slot(name), theme.palette.len()) {
        (None, _) | (_, 0) => theme.bg_control,
        (Some(slot), len) => mix(theme.bg_raised, theme.palette[slot % len], 0.35),
    }
}
