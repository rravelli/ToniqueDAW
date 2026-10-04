use std::collections::{HashMap, VecDeque};
use std::fmt;

use super::compiled::{CompiledGraph, CompiledNode};
use super::desc::GraphDescription;
use super::node::{Node, NodeIdentity, NodeProperties};
use super::topology::{GraphTopology, TopologyNode};
use crate::nodes::DelayNode;

/// Nodes are only taken out of `work` once placed, and each is placed once.
const LIVE: &str = "a node still being compiled";

#[derive(Clone, Copy, Debug)]
pub struct CompileOptions {
    pub sample_rate: f64,
    /// Largest block the graph will ever be asked to process.
    pub max_block: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompileError {
    NoOutput,
    InvalidNodeRef {
        node: usize,
        reference: usize,
    },
    /// Names of the nodes that sit on (or behind) a cycle.
    Cycle(Vec<&'static str>),
    DuplicateIdentity(NodeIdentity),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoOutput => write!(f, "graph has no output node"),
            Self::InvalidNodeRef { node, reference } => {
                write!(f, "node {node} references missing node {reference}")
            }
            Self::Cycle(names) => write!(f, "graph contains a cycle through: {}", names.join(", ")),
            Self::DuplicateIdentity(id) => write!(f, "two nodes share identity {id:?}"),
        }
    }
}

impl std::error::Error for CompileError {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompileStats {
    pub described: usize,
    pub pruned: usize,
    pub deduplicated: usize,
    pub delays_inserted: usize,
    pub scheduled: usize,
    pub buffer_slots: usize,
    /// Latency of the output node relative to the timeline.
    pub output_latency: usize,
}

struct WorkNode {
    node: Box<dyn Node>,
    props: NodeProperties,
    name: &'static str,
    label: Option<String>,
    owner: Option<u64>,
    inputs: Vec<usize>,
    after: Vec<usize>,
}

/// Compile a description into an executable graph. Runs entirely off the RT
/// thread; all allocation (buffers, delay lines, node `prepare`) happens here.
pub fn compile(
    desc: GraphDescription,
    opts: &CompileOptions,
) -> Result<CompiledGraph, CompileError> {
    let output = desc.output.ok_or(CompileError::NoOutput)?.0;
    let described = desc.nodes.len();
    let mut work: Vec<Option<WorkNode>> = Vec::with_capacity(described);
    for (i, spec) in desc.nodes.into_iter().enumerate() {
        for r in spec.inputs.iter().chain(&spec.after) {
            if r.0 >= described {
                return Err(CompileError::InvalidNodeRef {
                    node: i,
                    reference: r.0,
                });
            }
        }
        let props = spec.node.properties();
        let name = spec.node.name();
        work.push(Some(WorkNode {
            node: spec.node,
            props,
            name,
            label: spec.label,
            owner: spec.owner,
            inputs: spec.inputs.iter().map(|n| n.0).collect(),
            after: spec.after.iter().map(|n| n.0).collect(),
        }));
    }
    if output >= described {
        return Err(CompileError::NoOutput);
    }

    // 1. Prune everything the output doesn't (transitively) depend on.
    let mut reachable = vec![false; described];
    let mut stack = vec![output];
    while let Some(n) = stack.pop() {
        if std::mem::replace(&mut reachable[n], true) {
            continue;
        }
        let w = work[n].as_ref().expect(LIVE);
        stack.extend(
            w.inputs
                .iter()
                .chain(&w.after)
                .copied()
                .filter(|&p| !reachable[p]),
        );
    }
    let pruned = reachable.iter().filter(|r| !**r).count();
    for (i, r) in reachable.iter().enumerate() {
        if !r {
            work[i] = None;
        }
    }

    // 2. Topological sort (Kahn), rejecting cycles.
    let order = topological_sort(&work)?;

    // 3. Structural dedup: fold input content IDs into each node's own.
    let mut canon: Vec<usize> = (0..described).collect();
    let mut final_content: Vec<Option<u64>> = vec![None; described];
    let mut seen: HashMap<u64, usize> = HashMap::new();
    let mut deduplicated = 0;
    let mut deduped_order = Vec::with_capacity(order.len());
    for &i in &order {
        let w = work[i].as_mut().expect(LIVE);
        for p in w.inputs.iter_mut().chain(w.after.iter_mut()) {
            *p = canon[*p];
        }
        w.after.sort_unstable();
        w.after.dedup();
        let key = w.props.content_id.and_then(|own| {
            // Only dedupable if every input is itself content-addressed.
            let ins: Option<Vec<u64>> = w.inputs.iter().map(|&p| final_content[p]).collect();
            let afters: Option<Vec<u64>> = w.after.iter().map(|&p| final_content[p]).collect();
            let p = &w.props;
            Some(super::hash_of(&(
                own.0,
                ins?,
                afters?,
                p.channels,
                p.has_midi,
                p.latency_samples,
            )))
        });
        final_content[i] = key;
        if let Some(key) = key {
            if let Some(&existing) = seen.get(&key) {
                canon[i] = existing;
                work[i] = None;
                deduplicated += 1;
                continue;
            }
            seen.insert(key, i);
        }
        deduped_order.push(i);
    }
    let output = canon[output];

    // 4. Latency compensation: delay the earlier-arriving inputs at every
    //    join so all inputs are aligned; share delays per (source, amount).
    let mut latency: Vec<usize> = vec![0; work.len()];
    let mut delay_cache: HashMap<(usize, usize), usize> = HashMap::new();
    let mut final_order = Vec::with_capacity(deduped_order.len());
    let mut delays_inserted = 0;
    for &i in &deduped_order {
        let inputs = work[i].as_ref().expect(LIVE).inputs.clone();
        let max_in = inputs.iter().map(|&p| latency[p]).max().unwrap_or(0);
        let mut new_inputs = inputs.clone();
        for (slot, &p) in inputs.iter().enumerate() {
            let diff = max_in - latency[p];
            if diff == 0 {
                continue;
            }
            let d = *delay_cache.entry((p, diff)).or_insert_with(|| {
                let src = work[p].as_ref().expect(LIVE);
                let (owner, src) = (src.owner, &src.props);
                let node = DelayNode::new(diff, src.channels, src.has_midi);
                let props = node.properties();
                let label = Some(format!("latency comp. +{diff}"));
                work.push(Some(WorkNode {
                    name: node.name(),
                    label,
                    owner,
                    node: Box::new(node),
                    props,
                    inputs: vec![p],
                    after: vec![],
                }));
                latency.push(max_in);
                final_order.push(work.len() - 1);
                delays_inserted += 1;
                work.len() - 1
            });
            new_inputs[slot] = d;
        }
        let w = work[i].as_mut().expect(LIVE);
        w.inputs = new_inputs;
        latency[i] = max_in + w.props.latency_samples;
        final_order.push(i);
    }

    // 5. Compact to schedule indices.
    let n = final_order.len();
    let mut index_of = vec![usize::MAX; work.len()];
    for (k, &i) in final_order.iter().enumerate() {
        index_of[i] = k;
    }
    let mut nodes: Vec<WorkNode> = Vec::with_capacity(n);
    for &i in &final_order {
        let mut w = work[i].take().expect(LIVE);
        w.inputs.iter_mut().for_each(|p| *p = index_of[*p]);
        w.after.iter_mut().for_each(|p| *p = index_of[*p]);
        nodes.push(w);
    }
    let output_idx = index_of[output];
    let output_latency = latency[output];
    let total_latency: Vec<usize> = final_order.iter().map(|&i| latency[i]).collect();

    // Identities must be unique so state migration is unambiguous.
    let mut identity_index: Vec<(NodeIdentity, u32)> = nodes
        .iter()
        .enumerate()
        .filter_map(|(k, w)| w.props.identity.map(|id| (id, k as u32)))
        .collect();
    identity_index.sort_unstable();
    if let Some(w) = identity_index.windows(2).find(|w| w[0].0 == w[1].0) {
        return Err(CompileError::DuplicateIdentity(w[0].0));
    }

    // 6. Dependencies (distinct predecessors) for the scheduler.
    let preds: Vec<Vec<usize>> = nodes
        .iter()
        .map(|w| {
            let mut p: Vec<usize> = w.inputs.iter().chain(&w.after).copied().collect();
            p.sort_unstable();
            p.dedup();
            p
        })
        .collect();
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (k, ps) in preds.iter().enumerate() {
        for &p in ps {
            dependents[p].push(k);
        }
    }

    // 7. Static buffer plan.
    let (slot_of, slot_count, slot_channels) =
        allocate_buffers(&nodes, &preds, &dependents, output_idx);

    let stats = CompileStats {
        described,
        pruned,
        deduplicated,
        delays_inserted,
        scheduled: n,
        buffer_slots: slot_count,
        output_latency,
    };

    let topology = GraphTopology::new(
        nodes
            .iter()
            .zip(total_latency)
            .map(|(w, total_latency)| TopologyNode {
                name: w.name,
                label: w.label.clone(),
                owner: w.owner,
                channels: w.props.channels,
                has_midi: w.props.has_midi,
                latency_samples: w.props.latency_samples,
                total_latency,
                inputs: w.inputs.clone(),
                after: w.after.clone(),
            })
            .collect(),
        output_idx,
        stats,
    );

    // 8. Prepare (may allocate: we're still off the RT thread).
    let compiled_nodes = nodes
        .into_iter()
        .map(|mut w| {
            w.node.prepare(opts.sample_rate, opts.max_block);
            CompiledNode::new(w.node, w.name, w.props, w.inputs)
        })
        .collect();

    Ok(CompiledGraph::build(
        compiled_nodes,
        dependents,
        preds.iter().map(|p| p.len() as u32).collect(),
        slot_of,
        slot_count,
        slot_channels,
        identity_index,
        output_idx,
        *opts,
        stats,
        topology,
    ))
}

fn topological_sort(work: &[Option<WorkNode>]) -> Result<Vec<usize>, CompileError> {
    let len = work.len();
    let mut indegree = vec![0usize; len];
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); len];
    for (i, w) in work.iter().enumerate() {
        let Some(w) = w else { continue };
        let mut preds: Vec<usize> = w.inputs.iter().chain(&w.after).copied().collect();
        preds.sort_unstable();
        preds.dedup();
        indegree[i] = preds.len();
        for p in preds {
            succ[p].push(i);
        }
    }
    let mut queue: VecDeque<usize> = (0..len)
        .filter(|&i| work[i].is_some() && indegree[i] == 0)
        .collect();
    let mut order = Vec::with_capacity(len);
    while let Some(i) = queue.pop_front() {
        order.push(i);
        for &s in &succ[i] {
            indegree[s] -= 1;
            if indegree[s] == 0 {
                queue.push_back(s);
            }
        }
    }
    let live = work.iter().filter(|w| w.is_some()).count();
    if order.len() != live {
        let stuck = (0..len)
            .filter(|&i| indegree[i] > 0)
            .filter_map(|i| work[i].as_ref().map(|w| w.name))
            .collect();
        return Err(CompileError::Cycle(stuck));
    }
    Ok(order)
}

/// Assign each audio-producing node a physical buffer slot, reusing slots
/// like a register allocator.
///
/// Plain sequential liveness ("free after the last consumer in schedule
/// order") is not enough once nodes run in parallel: a later node could
/// overwrite a slot while an unrelated earlier consumer is still reading it
/// on another thread. So a slot is only handed to node `n` when *every*
/// consumer of its current occupant is an ancestor of `n` — i.e. guaranteed
/// by the dependency graph to have finished before `n` starts, under any
/// execution order. The plan is then valid for both executors.
fn allocate_buffers(
    nodes: &[WorkNode],
    preds: &[Vec<usize>],
    dependents: &[Vec<usize>],
    output: usize,
) -> (Vec<Option<usize>>, usize, usize) {
    let n = nodes.len();
    let words = n.div_ceil(64);
    // ancestors[i] = bitset of all nodes that must complete before i.
    let mut ancestors = vec![0u64; n * words];
    for (i, ps) in preds.iter().enumerate() {
        for &p in ps {
            let (before, rest) = ancestors.split_at_mut(i * words);
            let row = &mut rest[..words];
            let prow = &before[p * words..(p + 1) * words];
            for w in 0..words {
                row[w] |= prow[w];
            }
            row[p / 64] |= 1 << (p % 64);
        }
    }
    let is_ancestor = |a: usize, of: usize| ancestors[of * words + a / 64] & (1 << (a % 64)) != 0;

    let mut slot_of = vec![None; n];
    let mut occupant: Vec<usize> = Vec::new(); // slot -> node currently holding it
    let mut max_channels = 0;
    for i in 0..n {
        if nodes[i].props.channels == 0 {
            continue;
        }
        max_channels = max_channels.max(nodes[i].props.channels);
        let reusable = (0..occupant.len()).find(|&s| {
            let o = occupant[s];
            o != output
                && !dependents[o].is_empty()
                && dependents[o].iter().all(|&c| is_ancestor(c, i))
        });
        let slot = reusable.unwrap_or_else(|| {
            occupant.push(i);
            occupant.len() - 1
        });
        occupant[slot] = i;
        slot_of[i] = Some(slot);
    }
    (slot_of, occupant.len(), max_channels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{NodeId, ProcessContext};
    use crate::nodes::SumNode;

    /// Emits a constant on every channel; optionally reports latency.
    struct Dc {
        value: f32,
        latency: usize,
        dedup: bool,
    }

    impl Node for Dc {
        fn properties(&self) -> NodeProperties {
            let p = NodeProperties::audio(1).with_latency(self.latency);
            if self.dedup {
                p.with_content(super::super::ContentId::of(&self.value.to_bits()))
            } else {
                p
            }
        }
        fn process(&mut self, ctx: &mut ProcessContext) {
            ctx.audio_out.channel_mut(0).fill(self.value);
        }
    }

    fn dc(v: f32) -> Dc {
        Dc {
            value: v,
            latency: 0,
            dedup: false,
        }
    }

    const OPTS: CompileOptions = CompileOptions {
        sample_rate: 48000.0,
        max_block: 64,
    };

    fn render(g: &mut CompiledGraph, len: usize) -> Vec<f32> {
        g.process_sequential(&crate::graph::BlockInfo {
            block_len: len,
            sample_rate: 48000.0,
            timeline_pos: 0,
            playing: true,
            jumped: false,
        });
        g.output(len).channel(0).to_vec()
    }

    #[test]
    fn sums_and_prunes() {
        let mut d = GraphDescription::new();
        let a = d.add(dc(1.0), &[]);
        let b = d.add(dc(2.0), &[]);
        let _unused = d.add(dc(100.0), &[]);
        let s = d.add(SumNode::new(1), &[a, b]);
        d.set_output(s);
        let mut g = compile(d, &OPTS).unwrap();
        assert_eq!(g.stats().pruned, 1);
        assert_eq!(render(&mut g, 4), vec![3.0; 4]);
    }

    #[test]
    fn rejects_cycles_and_bad_refs() {
        let mut d = GraphDescription::new();
        let a = d.add(SumNode::new(1), &[]);
        let b = d.add(SumNode::new(1), &[a]);
        d.connect(b, a);
        d.set_output(b);
        assert!(matches!(compile(d, &OPTS), Err(CompileError::Cycle(n)) if n.len() == 2));

        let mut d = GraphDescription::new();
        let a = d.add(SumNode::new(1), &[NodeId(7)]);
        d.set_output(a);
        assert!(matches!(
            compile(d, &OPTS),
            Err(CompileError::InvalidNodeRef { .. })
        ));
        assert_eq!(
            compile(GraphDescription::new(), &OPTS).err(),
            Some(CompileError::NoOutput)
        );
    }

    #[test]
    fn dedups_identical_subgraphs_only() {
        let mut d = GraphDescription::new();
        let a = d.add(
            Dc {
                value: 1.0,
                latency: 0,
                dedup: true,
            },
            &[],
        );
        let b = d.add(
            Dc {
                value: 1.0,
                latency: 0,
                dedup: true,
            },
            &[],
        );
        let c = d.add(
            Dc {
                value: 2.0,
                latency: 0,
                dedup: true,
            },
            &[],
        );
        let s = d.add(SumNode::new(1), &[a, b, c]);
        d.set_output(s);
        let mut g = compile(d, &OPTS).unwrap();
        assert_eq!(g.stats().deduplicated, 1);
        assert_eq!(g.stats().scheduled, 3);
        // The surviving node is read twice: output unchanged by dedup.
        assert_eq!(render(&mut g, 2), vec![4.0; 2]);
    }

    #[test]
    fn inserts_latency_compensation() {
        let mut d = GraphDescription::new();
        let src = d.add(
            Dc {
                value: 1.0,
                latency: 0,
                dedup: false,
            },
            &[],
        );
        let lat = d.add(DelayNode::reporting(3, 1), &[src]);
        let dry = d.add(SumNode::new(1), &[src]);
        let s = d.add(SumNode::new(1), &[lat, dry]);
        d.set_output(s);
        let mut g = compile(d, &OPTS).unwrap();
        assert_eq!(g.stats().delays_inserted, 1);
        assert_eq!(g.stats().output_latency, 3);
        // Both branches arrive 3 samples late, together.
        assert_eq!(render(&mut g, 6), vec![0.0, 0.0, 0.0, 2.0, 2.0, 2.0]);
    }

    #[test]
    fn buffer_plan_reuses_slots_along_a_chain() {
        let mut d = GraphDescription::new();
        let mut prev = d.add(dc(1.0), &[]);
        for _ in 0..10 {
            prev = d.add(SumNode::new(1), &[prev]);
        }
        d.set_output(prev);
        let mut g = compile(d, &OPTS).unwrap();
        assert_eq!(g.stats().buffer_slots, 2);
        assert_eq!(render(&mut g, 3), vec![1.0; 3]);
    }

    #[test]
    fn buffer_plan_is_parallel_safe() {
        // a feeds b and c (independent). A slot freed by b must not be
        // reused by c's sibling d while c may still be reading a.
        let mut d = GraphDescription::new();
        let a = d.add(dc(1.0), &[]);
        let b = d.add(SumNode::new(1), &[a]);
        let c = d.add(SumNode::new(1), &[a]);
        let e = d.add(dc(5.0), &[]);
        let s = d.add(SumNode::new(1), &[b, c, e]);
        d.set_output(s);
        let g = compile(d, &OPTS).unwrap();
        let slots = g.slot_assignment();
        // `e` has no ordering relation with b or c, so it can't take a's slot.
        assert_ne!(
            slots[g.index_of_name_nth("Dc", 1)],
            slots[g.index_of_name_nth("Dc", 0)]
        );
    }
}
