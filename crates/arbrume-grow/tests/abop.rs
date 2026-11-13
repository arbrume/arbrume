//! Regression fixtures: the tree models of Honda and of Aono and Kunii as given in
//! chapter 2 of Prusinkiewicz and Lindenmayer, "The Algorithmic Beauty of Plants" (1990),
//! figures 2.6 and 2.7 with the constants of tables 2.1 and 2.2.
//!
//! Both models are run through the growth cycle of this crate. The angles, the
//! phyllotaxis and the branching mode come from a species; the lengths come from a bud
//! fate that reproduces the productions: every apex adds one segment per derivation
//! step, the straight (or relay) daughter being `r1` times its mother and the lateral
//! daughter `r2` times its mother.
//!
//! Expected values are derived from the productions. After `n` steps:
//!
//! * step `k` adds `2^(k-1)` segments, `2^n - 1` in all;
//! * the segments of branching order `o` (the straight or relay daughter keeps the
//!   order, the lateral daughter adds one) number `C(n, o + 1)`;
//! * each apex of length `l` yields a segment `l` and apices `l * r1` and `l * r2`, so
//!   the total length is the sum of `(r1 + r2)^d` for `d` in `0..n`.
//!
//! Table 2.1 gives `a2 = -30` for figure 2.6d. The sign only mirrors the tree, so the
//! fixture uses 30.

use arbrume_core::curve::OrderCurve;
use arbrume_core::graph::{ChainId, NodeId, PlantGraph};
use arbrume_core::species::{AxisCategory, AxisOrientation, Branching, Phyllotaxis, Species};
use arbrume_grow::{
    BudFate, BudSite, Context, Flush, MeristemId, MeristemState, NoEnvironment, NoShedding, Plant,
    SiteKind, Stages, Straight, grow_with,
};
use glam::Vec3;

/// Derivation length of the listings of figures 2.6 and 2.7.
const STEPS: u16 = 10;

/// Bud fate of both models: one segment per apex and per step, contracted by `r1` for a
/// straight or relay daughter and by `r2` for a lateral daughter.
#[derive(Debug)]
struct ContractionFate {
    r1: f32,
    r2: f32,
}

impl BudFate for ContractionFate {
    fn admit(&mut self, _plant: &Plant, _site: &BudSite) -> Option<f32> {
        Some(f32::MAX)
    }

    fn decide(&mut self, plant: &Plant, _cycle: u16, from: MeristemId, flushes: &mut Vec<Flush>) {
        if from.0 > 0 {
            return;
        }
        let graph = &plant.graph;
        for (index, meristem) in plant.meristems.iter().enumerate() {
            let mother = |node| graph.internode_length(node);
            let length = match (meristem.state, meristem.kind) {
                (MeristemState::Done | MeristemState::Relayed, _) => continue,
                (MeristemState::Resting, SiteKind::Seed) => 1.0,
                (MeristemState::Growing, _) => self.r1 * mother(graph.chain(meristem.chain).last),
                (MeristemState::Resting, SiteKind::Relay) => {
                    self.r1 * mother(graph.bud(meristem.bud).node)
                }
                (MeristemState::Resting, SiteKind::Lateral) => {
                    self.r2 * mother(graph.bud(meristem.bud).node)
                }
            };
            flushes.push(Flush {
                meristem: MeristemId(index as u32),
                metamers: 1,
                length,
                last: false,
            });
        }
    }
}

fn plagiotropic() -> AxisCategory {
    AxisCategory {
        orientation: AxisOrientation::Plagiotropic,
        ..Default::default()
    }
}

/// Species shared by both fixtures: exact angles, Leonardo radii, no leaves.
fn skeleton_species() -> Species {
    let mut species = Species::default();
    species.architecture.max_order = 12;
    species.growth.branching_angle_variation = OrderCurve::constant(0.0);
    species.geometry.pipe_exponent = 2.0;
    species.foliage.enabled = false;
    species
}

/// Monopodial model of figure 2.6: the trunk bears one lateral per node at `a0` with
/// divergence 137.5 degrees; lateral axes bear one lateral per node at `a2`,
/// alternately left and right in the plane closest to horizontal.
fn honda(r1: f32, r2: f32, a0: f32, a2: f32) -> PlantGraph {
    let mut species = skeleton_species();
    species.architecture.phyllotaxis = Phyllotaxis::distichous();
    species.architecture.axes = vec![
        AxisCategory {
            phyllotaxis: Some(Phyllotaxis::spiral()),
            ..Default::default()
        },
        plagiotropic(),
    ];
    species.growth.branching_angle = OrderCurve::constants(&[a0, a0, a2]).unwrap();
    run(&species, r1, r2)
}

/// Sympodial model of figure 2.7: every apex ends after one segment, leaving a relay at
/// `a1` and a lateral at `a2` on the opposite side, in the plane closest to horizontal.
fn aono_kunii(r1: f32, r2: f32, a1: f32, a2: f32) -> PlantGraph {
    let mut species = skeleton_species();
    species.architecture.branching = Branching::Sympodial;
    species.architecture.phyllotaxis = Phyllotaxis {
        leaves_per_node: 2,
        divergence_deg: 180.0,
    };
    species.architecture.axes = vec![plagiotropic()];
    species.growth.relay_angle = OrderCurve::constant(a1);
    species.growth.branching_angle = OrderCurve::constant(a2);
    run(&species, r1, r2)
}

fn run(species: &Species, r1: f32, r2: f32) -> PlantGraph {
    species.validate().unwrap();
    let cx = Context {
        species,
        seed: 1,
        age: STEPS,
        scale: 1.0,
    };
    let mut stages = Stages {
        environment: NoEnvironment,
        fate: ContractionFate { r1, r2 },
        orientation: Straight,
        shedding: NoShedding,
    };
    let graph = grow_with(&cx, &mut stages);
    graph.check_invariants().unwrap();
    assert!(graph.is_contiguous());
    graph
}

fn binomial(n: u64, k: u64) -> u64 {
    (0..k).fold(1, |value, i| value * (n - i) / (i + 1))
}

fn direction(graph: &PlantGraph, node: NodeId) -> Vec3 {
    (graph.position(node) - graph.proximal(node)).normalize()
}

fn angle_deg(a: Vec3, b: Vec3) -> f32 {
    a.dot(b).clamp(-1.0, 1.0).acos().to_degrees()
}

fn close(value: f32, expected: f32, tolerance: f32) -> bool {
    (value - expected).abs() <= tolerance
}

/// Checks shared by both models: segment counts per step and per order, total length
/// and the rule of Leonardo on the radii.
fn check_counts_and_lengths(graph: &PlantGraph, r1: f32, r2: f32) {
    let steps = u64::from(STEPS);
    assert_eq!(graph.node_count() as u64, (1 << steps) - 1);
    for step in 0..STEPS {
        let born = graph.births().iter().filter(|&&birth| birth == step);
        assert_eq!(born.count(), 1 << step, "segments added at step {step}");
    }
    for order in 0..STEPS {
        let of_order = graph.orders().iter().filter(|&&o| u16::from(o) == order);
        let expected = binomial(steps, u64::from(order) + 1);
        assert_eq!(of_order.count() as u64, expected, "order {order}");
    }
    let lengths = graph.node_ids().map(|node| graph.internode_length(node));
    let total: f32 = lengths.sum();
    let expected: f32 = (0..i32::from(STEPS)).map(|d| (r1 + r2).powi(d)).sum();
    assert!(
        close(total, expected, expected * 1e-4),
        "{total} vs {expected}"
    );

    let children = graph.children();
    let tip = Species::default().geometry.tip_radius;
    for node in graph.node_ids() {
        let radius = graph.radii()[node.index()];
        let daughters = children.of(node).iter();
        let below: f32 = daughters.map(|d| graph.radii()[d.index()].powi(2)).sum();
        let expected = if below > 0.0 { below.sqrt() } else { tip };
        assert!(close(radius, expected, expected * 1e-4));
    }
}

/// Contraction ratio and branching angle of every segment against its mother segment.
/// `lateral` lists the angle of lateral daughters by their order, starting at order 1.
fn check_daughters(graph: &PlantGraph, ratios: (f32, f32), straight: f32, lateral: &[f32]) {
    for node in graph.node_ids() {
        let Some(mother) = graph.parent(node) else {
            continue;
        };
        let order = usize::from(graph.orders()[node.index()]);
        let same_order = order == usize::from(graph.orders()[mother.index()]);
        let ratio = graph.internode_length(node) / graph.internode_length(mother);
        let angle = angle_deg(direction(graph, node), direction(graph, mother));
        let (expected_ratio, expected_angle) = if same_order {
            (ratios.0, straight)
        } else {
            (ratios.1, lateral[(order - 1).min(lateral.len() - 1)])
        };
        assert!(close(ratio, expected_ratio, 1e-4), "ratio {ratio}");
        assert!(close(angle, expected_angle, 0.05), "angle {angle}");
    }
}

/// Unit component of the direction of `node` orthogonal to the direction of its mother.
fn offset(graph: &PlantGraph, node: NodeId) -> Vec3 {
    let mother = direction(graph, graph.parent(node).unwrap());
    let own = direction(graph, node);
    (own - mother * own.dot(mother)).normalize()
}

#[test]
fn honda_monopodial_trees_match_table_2_1() {
    let rows = [
        (0.9, 0.6, 45.0, 45.0),
        (0.9, 0.9, 45.0, 45.0),
        (0.9, 0.8, 45.0, 45.0),
        (0.9, 0.7, 30.0, 30.0),
    ];
    for (r1, r2, a0, a2) in rows {
        let graph = honda(r1, r2, a0, a2);
        check_counts_and_lengths(&graph, r1, r2);
        check_daughters(&graph, (r1, r2), 0.0, &[a0, a2]);

        let trunk = graph.chain(ChainId::new(0));
        assert_eq!((trunk.order, trunk.len), (0, u32::from(STEPS)));
        let height: f32 = (0..i32::from(STEPS)).map(|k| r1.powi(k)).sum();
        let top = graph.bounds().unwrap().max.y;
        assert!(close(top, height, 1e-4), "height {top} vs {height}");
        for order in 0..STEPS {
            let axes = graph
                .chains()
                .iter()
                .filter(|c| u16::from(c.order) == order);
            let expected = binomial(u64::from(STEPS) - 1, u64::from(order));
            assert_eq!(axes.count() as u64, expected, "axes of order {order}");
        }
        check_honda_planes(&graph);
    }
}

/// First nodes of the chains of `order`, sorted by attachment node.
fn first_nodes(graph: &PlantGraph, order: u8) -> Vec<NodeId> {
    let chains = graph.chains().iter().filter(|c| c.order == order);
    let mut nodes: Vec<NodeId> = chains.map(|c| c.first).collect();
    nodes.sort_by_key(|&node| graph.parent(node));
    nodes
}

/// Laterals of the trunk turn by the divergence angle from one to the next; laterals of
/// the other axes lie in the plane closest to horizontal and alternate sides.
fn check_honda_planes(graph: &PlantGraph) {
    for pair in first_nodes(graph, 1).windows(2) {
        let (a, b) = (offset(graph, pair[0]), offset(graph, pair[1]));
        assert!(close(angle_deg(a, b), 137.5, 0.05));
    }
    let on_branches = first_nodes(graph, 2);
    for &node in &on_branches {
        assert!(offset(graph, node).y.abs() < 1e-4);
    }
    for pair in on_branches.windows(2) {
        let mothers = (graph.parent(pair[0]), graph.parent(pair[1]));
        if mothers.1.and_then(|mother| graph.parent(mother)) == mothers.0 {
            let sides = offset(graph, pair[0]).dot(offset(graph, pair[1]));
            assert!(close(sides, -1.0, 1e-3), "successive laterals on one side");
        }
    }
}

#[test]
fn aono_kunii_sympodial_trees_match_table_2_2() {
    let rows = [
        (0.9, 0.7, 5.0, 65.0),
        (0.9, 0.7, 10.0, 60.0),
        (0.9, 0.8, 20.0, 50.0),
        (0.9, 0.8, 35.0, 35.0),
    ];
    for (r1, r2, a1, a2) in rows {
        let graph = aono_kunii(r1, r2, a1, a2);
        check_counts_and_lengths(&graph, r1, r2);
        check_daughters(&graph, (r1, r2), a1, &[a2]);
        assert_eq!(graph.chain_count(), graph.node_count());

        let children = graph.children();
        for node in graph.node_ids() {
            let &[first, second] = children.of(node) else {
                assert_eq!(children.of(node), []);
                continue;
            };
            let (a, b) = (offset(&graph, first), offset(&graph, second));
            assert!(close(a.dot(b), -1.0, 1e-3), "daughters on the same side");
            if graph.parent(node).is_some() {
                assert!(a.y.abs() < 1e-4 && b.y.abs() < 1e-4);
            }
        }
    }
}
