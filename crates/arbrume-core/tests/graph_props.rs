//! Property tests of the plant graph on randomly grown plants.

use arbrume_core::graph::{
    BudId, BudKind, BudState, ChainId, ChainKind, Metamer, NodeId, PlantGraph,
};
use glam::Vec3;
use proptest::prelude::*;

/// One random growth step: an operation selector, a target selector and a variant flag.
type Op = (u8, u32, bool);

/// Next metamer: its x coordinate is the creation index of the node, a tag that survives
/// reordering, and births never decrease with creation.
fn next_metamer(graph: &PlantGraph) -> Metamer {
    let index = graph.node_count();
    let position = Vec3::new(index as f32, (index % 7) as f32, (index % 3) as f32);
    Metamer::new(position, 0.01, (index / 5) as u16)
}

fn pick(selector: u32, count: usize) -> usize {
    selector as usize % count
}

fn sprout_or_add_root(graph: &mut PlantGraph, selector: u32, relay: bool) {
    let lateral: Vec<BudId> = graph
        .bud_ids()
        .filter(|&bud| {
            let bud = graph.bud(bud);
            bud.kind == BudKind::Lateral && bud.state != BudState::Dead
        })
        .collect();
    let metamer = next_metamer(graph);
    if lateral.is_empty() {
        graph.add_root_chain(Vec3::new(-1.0, 0.0, selector as f32), metamer);
    } else {
        let kind = if relay {
            ChainKind::Relay
        } else {
            ChainKind::Lateral
        };
        graph.sprout(lateral[pick(selector, lateral.len())], kind, metamer);
    }
}

fn apply(graph: &mut PlantGraph, (op, selector, variant): Op) {
    let metamer = next_metamer(graph);
    match op % 8 {
        0..=3 => {
            graph.extend_chain(ChainId::new(pick(selector, graph.chain_count())), metamer);
        }
        4 | 5 => {
            let attach = NodeId::new(pick(selector, graph.node_count()));
            let kind = if variant {
                ChainKind::Relay
            } else {
                ChainKind::Lateral
            };
            let node = graph.add_chain(attach, kind, metamer);
            if op % 8 == 5 {
                graph.add_bud(node, BudKind::Terminal, Vec3::Y);
            }
        }
        6 => {
            let node = NodeId::new(pick(selector, graph.node_count()));
            graph.add_bud(node, BudKind::Lateral, Vec3::X);
        }
        _ => sprout_or_add_root(graph, selector, variant),
    }
}

fn grow(ops: &[Op]) -> PlantGraph {
    let mut graph = PlantGraph::new();
    let base = graph.add_root_chain(Vec3::new(0.0, -1.0, 0.0), next_metamer(&graph));
    graph.add_bud(base, BudKind::Terminal, Vec3::Y);
    for &op in ops {
        apply(&mut graph, op);
    }
    graph
}

fn plants() -> impl Strategy<Value = PlantGraph> {
    prop::collection::vec((any::<u8>(), any::<u32>(), any::<bool>()), 0..120)
        .prop_map(|ops| grow(&ops))
}

/// Creation index of a node, read back from its position.
fn tag(graph: &PlantGraph, node: NodeId) -> u32 {
    graph.position(node).x as u32
}

/// Parent tag, chain and order of every node, indexed by tag: the topology of the plant
/// independent of node ids.
fn topology(graph: &PlantGraph) -> Vec<(Option<u32>, ChainId, u8)> {
    let mut rows = vec![(None, ChainId::NONE, 0); graph.node_count()];
    for node in graph.node_ids() {
        let parent = graph.parent(node).map(|parent| tag(graph, parent));
        rows[tag(graph, node) as usize] =
            (parent, graph.chain_of(node), graph.orders()[node.index()]);
    }
    rows
}

fn subtree_sizes(graph: &PlantGraph) -> Vec<u32> {
    let mut sizes = vec![1_u32; graph.node_count()];
    graph.accumulate_basipetal(&mut sizes, |parent, child| *parent += *child);
    sizes
}

proptest! {
    #[test]
    fn grown_plants_satisfy_the_invariants(graph in plants()) {
        prop_assert_eq!(graph.check_invariants(), Ok(()));
    }

    #[test]
    fn parents_come_first_and_every_node_reaches_a_root(graph in plants()) {
        for node in graph.node_ids() {
            let mut current = node;
            let mut steps = 0;
            while let Some(parent) = graph.parent(current) {
                prop_assert!(parent < current);
                current = parent;
                steps += 1;
                prop_assert!(steps <= graph.node_count());
            }
            let root_chain = graph.chain(graph.chain_of(current));
            prop_assert!(root_chain.attach.is_none() && root_chain.first == current);
        }
    }

    #[test]
    fn chains_partition_the_nodes(graph in plants()) {
        let mut seen = vec![0_u32; graph.node_count()];
        for chain in graph.chain_ids() {
            let nodes: Vec<NodeId> = graph.chain_nodes_rev(chain).collect();
            prop_assert_eq!(nodes.len(), graph.chain(chain).len as usize);
            prop_assert_eq!(nodes.first().copied(), Some(graph.chain(chain).last));
            prop_assert_eq!(nodes.last().copied(), Some(graph.chain(chain).first));
            for node in nodes {
                prop_assert_eq!(graph.chain_of(node), chain);
                seen[node.index()] += 1;
            }
        }
        prop_assert!(seen.iter().all(|&count| count == 1));
    }

    #[test]
    fn contiguity_flag_is_truthful(graph in plants()) {
        for chain in graph.chain_ids() {
            let Some(range) = graph.chain_range(chain) else {
                prop_assert!(!graph.is_contiguous());
                continue;
            };
            let mut expected: Vec<NodeId> = graph.chain_nodes_rev(chain).collect();
            expected.reverse();
            let actual: Vec<NodeId> = range.map(NodeId::new).collect();
            prop_assert_eq!(actual, expected);
        }
    }

    #[test]
    fn compaction_makes_chains_contiguous_and_keeps_the_plant(graph in plants()) {
        let mut compacted = graph.clone();
        let remap = compacted.compact();
        prop_assert_eq!(compacted.check_invariants(), Ok(()));
        prop_assert!(compacted.is_contiguous());
        prop_assert_eq!(topology(&compacted), topology(&graph));
        prop_assert_eq!(compacted.chain_count(), graph.chain_count());
        for chain in compacted.chain_ids() {
            let range = compacted.chain_range(chain).unwrap();
            let mut expected: Vec<NodeId> = compacted.chain_nodes_rev(chain).collect();
            expected.reverse();
            prop_assert_eq!(range.map(NodeId::new).collect::<Vec<_>>(), expected);
            let view = compacted.chain_view(chain).unwrap();
            prop_assert_eq!(view.len(), graph.chain(chain).len as usize);
            prop_assert_eq!(view.origin, graph.chain_origin(chain));
        }
        for bud in graph.bud_ids() {
            prop_assert_eq!(compacted.bud_position(bud), graph.bud_position(bud));
            prop_assert_eq!(compacted.bud(bud).kind, graph.bud(bud).kind);
        }
        match remap {
            None => prop_assert_eq!(&compacted, &graph),
            Some(remap) => {
                for old in graph.node_ids() {
                    prop_assert_eq!(compacted.position(remap[old.index()]), graph.position(old));
                }
            }
        }
        let identity: Vec<NodeId> = compacted.node_ids().collect();
        prop_assert_eq!(compacted.depth_first_order(), identity);
        prop_assert_eq!(compacted.clone().compact(), None);
    }

    #[test]
    fn serialization_order_is_depth_first(graph in plants()) {
        let order = graph.depth_first_order();
        let mut place = vec![usize::MAX; graph.node_count()];
        for (index, node) in order.iter().enumerate() {
            prop_assert_eq!(place[node.index()], usize::MAX, "node listed twice");
            place[node.index()] = index;
        }
        let sizes = subtree_sizes(&graph);
        for node in graph.node_ids() {
            let Some(parent) = graph.parent(node) else { continue };
            let start = place[parent.index()];
            let end = start + sizes[parent.index()] as usize;
            prop_assert!(start < place[node.index()] && place[node.index()] < end);
        }
        for chain in graph.chain_ids() {
            let places: Vec<usize> =
                graph.chain_nodes_rev(chain).map(|node| place[node.index()]).collect();
            prop_assert!(places.windows(2).all(|pair| pair[0] == pair[1] + 1));
        }
    }

    #[test]
    fn basipetal_accumulation_counts_descendants(graph in plants()) {
        let sizes = subtree_sizes(&graph);
        for ancestor in graph.node_ids() {
            let descendants =
                graph.node_ids().filter(|&node| graph.is_ancestor(ancestor, node)).count();
            prop_assert_eq!(sizes[ancestor.index()] as usize, descendants);
        }
        let mut depth = vec![0_u32; graph.node_count()];
        graph.propagate_acropetal(&mut depth, |parent, child| *child = *parent + 1);
        for node in graph.node_ids() {
            let expected = graph.parent(node).map_or(0, |parent| depth[parent.index()] + 1);
            prop_assert_eq!(depth[node.index()], expected);
        }
    }

    #[test]
    fn radii_follow_the_pipe_model(
        graph in plants(),
        exponent in 1.5_f32..3.5,
        tip in 0.001_f32..0.1,
    ) {
        let mut graph = graph;
        graph.pipe_radii(exponent, tip);
        prop_assert_eq!(graph.check_invariants(), Ok(()));
        let children = graph.children();
        for node in graph.node_ids() {
            let radius = graph.radii()[node.index()];
            let kids = children.of(node);
            let expected = if kids.is_empty() {
                tip
            } else {
                let sum: f32 = kids.iter().map(|k| graph.radii()[k.index()].powf(exponent)).sum();
                sum.powf(exponent.recip())
            };
            prop_assert!((radius - expected).abs() <= 1e-3 * expected, "{radius} vs {expected}");
            for kid in kids {
                prop_assert_eq!(graph.parent(*kid), Some(node));
                prop_assert!(graph.radii()[kid.index()] <= radius * 1.001);
            }
        }
    }

    #[test]
    fn shedding_covers_exactly_the_subtree(graph in plants(), selector in any::<u32>()) {
        let mut graph = graph;
        let cut = NodeId::new(pick(selector, graph.node_count()));
        graph.shed_subtree(cut);
        prop_assert_eq!(graph.check_invariants(), Ok(()));
        for node in graph.node_ids() {
            prop_assert_eq!(graph.is_shed(node), graph.is_ancestor(cut, node));
        }
        for bud in graph.bud_ids() {
            let bud = graph.bud(bud);
            prop_assert!(!graph.is_shed(bud.node) || bud.state == BudState::Dead);
        }
        graph.ensure_contiguous();
        for chain in graph.chain_ids() {
            let live = graph.chain_nodes_rev(chain).filter(|&node| !graph.is_shed(node)).count();
            prop_assert_eq!(graph.chain_view(chain).unwrap().len(), live);
        }
    }
}
