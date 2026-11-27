//! Where the graph view puts the engine's nodes, and what it writes on
//! them: shared by both apps' graph views.

use std::collections::HashMap;

use tonique_engine::graph::{GraphTopology, TopologyNode};

use crate::{
    project::ProjectState,
    utils::{color::Color, display_name},
};

// Sizes in graph units (scaled by the view's zoom).
pub const NODE_WIDTH: f32 = 176.;
pub const NODE_HEIGHT: f32 = 52.;
pub const COLUMN_WIDTH: f32 = 240.;
pub const ROW_HEIGHT: f32 = 72.;
/// Level meters show -60..0 dBFS.
const METER_RANGE_DB: f32 = 60.;

/// Each node's top left corner, in graph units, by node index.
///
/// Columns by longest path from the sources (so every edge points right),
/// then each node moved as far right as its consumers allow, so sources and
/// automation sit next to what they feed. Rows are ordered to keep connected
/// nodes close.
pub fn layout(topology: &GraphTopology) -> Vec<(f32, f32)> {
    let count = topology.nodes.len();
    let predecessors = |index: usize| {
        topology.nodes[index]
            .inputs
            .iter()
            .chain(&topology.nodes[index].after)
            .copied()
    };
    let mut successors = vec![Vec::new(); count];
    for index in 0..count {
        for predecessor in predecessors(index) {
            successors[predecessor].push(index);
        }
    }
    // Nodes are in schedule order, so predecessors come first.
    let mut depth = vec![0usize; count];
    for index in 0..count {
        depth[index] = predecessors(index)
            .map(|predecessor| depth[predecessor] + 1)
            .max()
            .unwrap_or(0);
    }
    let columns = depth.iter().max().map_or(0, |deepest| deepest + 1);
    for index in (0..count).rev() {
        if let Some(latest) = successors[index]
            .iter()
            .map(|&successor| depth[successor])
            .min()
        {
            depth[index] = latest - 1;
        }
    }
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); columns];
    for index in 0..count {
        rows[depth[index]].push(index);
    }

    // Barycenter sweeps: order each column by the mean row of its neighbours.
    let mut row_of = vec![0f32; count];
    let refresh = |rows: &Vec<Vec<usize>>, row_of: &mut Vec<f32>| {
        for column in rows {
            for (row, &index) in column.iter().enumerate() {
                row_of[index] = row as f32 - (column.len() as f32 - 1.) / 2.;
            }
        }
    };
    refresh(&rows, &mut row_of);
    for sweep in 0..6 {
        let forward = sweep % 2 == 0;
        let order: Vec<usize> = if forward {
            (1..columns).collect()
        } else {
            (0..columns.saturating_sub(1)).rev().collect()
        };
        for column in order {
            let key = |index: usize| {
                let neighbours: Vec<f32> = if forward {
                    predecessors(index).map(|other| row_of[other]).collect()
                } else {
                    successors[index]
                        .iter()
                        .map(|&other| row_of[other])
                        .collect()
                };
                if neighbours.is_empty() {
                    row_of[index]
                } else {
                    neighbours.iter().sum::<f32>() / neighbours.len() as f32
                }
            };
            let mut keyed: Vec<(f32, usize)> = rows[column]
                .iter()
                .map(|&index| (key(index), index))
                .collect();
            keyed.sort_by(|first, second| first.0.total_cmp(&second.0));
            rows[column] = keyed.into_iter().map(|(_, index)| index).collect();
            refresh(&rows, &mut row_of);
        }
    }

    (0..count)
        .map(|index| {
            (
                depth[index] as f32 * COLUMN_WIDTH,
                row_of[index] * ROW_HEIGHT,
            )
        })
        .collect()
}

/// The box around every node, in graph units: left, top, right, bottom.
pub fn bounds(positions: &[(f32, f32)]) -> Option<(f32, f32, f32, f32)> {
    positions
        .iter()
        .map(|&(left, top)| (left, top, left + NODE_WIDTH, top + NODE_HEIGHT))
        .reduce(|first, second| {
            (
                first.0.min(second.0),
                first.1.min(second.1),
                first.2.max(second.2),
                first.3.max(second.3),
            )
        })
}

/// The engine's name, displayed name and colour of each track and group,
/// by owner ID.
pub fn track_names(state: &ProjectState) -> HashMap<u64, (String, String, Color)> {
    state
        .tracks()
        .chain(state.groups())
        .map(|track| {
            (
                track.id.0,
                (
                    track.name.clone(),
                    display_name(&track.name, track.first_track_index),
                    track.color,
                ),
            )
        })
        .collect()
}

/// A node's title with its track's displayed name (the engine only knows
/// the raw name, e.g. `# Audio Track`), and the track's colour.
pub fn node_title(
    node: &TopologyNode,
    owners: &HashMap<u64, (String, String, Color)>,
) -> (String, Option<Color>) {
    let label = node.label.as_deref().unwrap_or(node.name);
    let Some((raw, shown, color)) = node.owner.and_then(|owner| owners.get(&owner)) else {
        return (label.to_string(), None);
    };
    let title = match label.strip_prefix(raw.as_str()) {
        Some(rest) => format!("{shown}{rest}"),
        None => label.to_string(),
    };
    (title, Some(*color))
}

/// Which palette colour tints a node, by what it does; `None` for the
/// plain control colour (sums).
pub fn node_palette_slot(name: &str) -> Option<usize> {
    Some(match name {
        "AudioClipNode" | "MidiClipNode" => 6,
        "VolumePanNode" => 4,
        "SumNode" => return None,
        "DelayNode" => 1,
        "AutomationNode" => 2,
        "MetronomeNode" => 5,
        _ => 7, // plugins
    })
}

/// A peak, 0..=1 on the meters' dB scale.
pub fn meter_unit(peak: f32) -> f32 {
    if peak <= 0. {
        return 0.;
    }
    ((20. * peak.log10() + METER_RANGE_DB) / METER_RANGE_DB).clamp(0., 1.)
}

/// `text` cut to `max` characters, with an ellipsis when cut.
pub fn elide(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max - 1).collect::<String>())
    }
}

/// The line of statistics over the graph.
pub fn stats_line(topology: &GraphTopology, engine_load: f32, nodes_cpu: f32) -> String {
    let stats = topology.stats;
    format!(
        "{} nodes · {} merged · {} pruned · {} latency delays · {} buffers · output latency {} smp · callback load {:.0}% · nodes {:.1}%",
        stats.scheduled,
        stats.deduplicated,
        stats.pruned,
        stats.delays_inserted,
        stats.buffer_slots,
        stats.output_latency,
        engine_load * 100.,
        nodes_cpu * 100.,
    )
}

/// What a hovered node's details say, line by line: its title first when
/// it has a label.
pub fn node_details(node: &TopologyNode, title: &str, level: f32, cpu: f32) -> Vec<String> {
    let mut lines = Vec::new();
    if node.label.is_some() {
        lines.push(title.to_string());
    }
    lines.push(format!("Type: {}", node.name));
    lines.push(format!(
        "Channels: {}{}",
        node.channels,
        if node.has_midi { " + MIDI" } else { "" }
    ));
    lines.push(format!(
        "Latency: {} smp (total {} smp)",
        node.latency_samples, node.total_latency
    ));
    let decibels = if level > 0. {
        20. * level.log10()
    } else {
        f32::NEG_INFINITY
    };
    lines.push(format!("Peak: {decibels:.1} dBFS"));
    lines.push(format!("CPU: {:.2}% of a core", cpu * 100.));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tonique_engine::{
        edit::{Clip, Edit, EditSession, Plugin, PluginKind, Track},
        engine::{Engine, EngineConfig},
        nodes::FilterMode,
        sample::SampleBuffer,
        time::BeatPos,
    };

    #[test]
    fn layout_puts_every_edge_left_to_right_without_overlaps() {
        let mut edit = Edit::new(120.);
        let source = edit.add_source(Arc::new(SampleBuffer::new(vec![vec![0.; 100]], 48000.)));
        for name in ["a", "b", "c"] {
            let mut track = Track::new(&mut edit, name);
            let clip = Clip::audio(&mut edit, BeatPos(0.), 1., source);
            track.clips.push(clip);
            let filter = Plugin::new(&mut edit, PluginKind::Filter(FilterMode::LowPass));
            track.channel.plugins.push(filter);
            edit.tracks.push(track);
        }
        let (engine, _process) = Engine::new(EngineConfig::default());
        let session = EditSession::new(edit, engine).unwrap();
        let topology = session.engine().graph_topology().unwrap();
        let positions = layout(&topology);

        for (index, node) in topology.nodes.iter().enumerate() {
            for &from in node.inputs.iter().chain(&node.after) {
                assert!(
                    positions[from].0 < positions[index].0,
                    "edge {from} -> {index} goes backwards"
                );
            }
        }
        let output = positions[topology.output];
        assert!(
            positions.iter().all(|position| position.0 <= output.0),
            "output is rightmost"
        );
        let overlap = |first: (f32, f32), second: (f32, f32)| {
            (first.0 - second.0).abs() < NODE_WIDTH && (first.1 - second.1).abs() < NODE_HEIGHT
        };
        for (index, first) in positions.iter().enumerate() {
            for second in &positions[index + 1..] {
                assert!(!overlap(*first, *second));
            }
        }
    }

    #[test]
    fn titles_use_displayed_track_names_and_colours() {
        let (engine, _process) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        state.set_track_palette(&[Color::rgb(200, 0, 0), Color::rgb(0, 200, 0)]);
        state.add_track();
        state.add_track();
        state.set_monitor_graph(true);
        state.update();
        let topology = state.graph().topology().cloned().expect("graph published");

        let owners = track_names(&state);
        let tracks: Vec<_> = state.tracks().collect();
        assert_ne!(tracks[0].color, tracks[1].color);
        for track in &tracks {
            let faders: Vec<_> = topology
                .nodes
                .iter()
                .filter(|node| node.owner == Some(track.id.0) && node.name == "VolumePanNode")
                .map(|node| node_title(node, &owners))
                .collect();
            let expected = format!("{} Audio Track · fader", track.first_track_index + 1);
            assert_eq!(faders, [(expected, Some(track.color))]);
        }
    }

    #[test]
    fn meter_scale() {
        assert_eq!(meter_unit(0.), 0.);
        assert_eq!(meter_unit(1.), 1.);
        assert!(meter_unit(0.001).abs() < 1e-6); // -60 dB
        assert!((meter_unit(10f32.powf(-30. / 20.)) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn elides_long_text() {
        assert_eq!(elide("short", 26), "short");
        assert_eq!(elide("abcdef", 4), "abc…");
    }
}
