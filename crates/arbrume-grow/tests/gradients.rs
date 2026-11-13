//! The gradient curves and architectural traits of a species shape the plant as
//! documented: silhouette, branching probability, angles, node density, phyllotaxis,
//! tiers, forks, relays, curvature and leaf retention.
//!
//! The tests grow a deliberately plain plant: a straight vertical trunk of 10 m in 0.1 m
//! internodes that takes 10 cycles to grow, with straight horizontal laterals of order 1,
//! no random variation and no leaves. Each test changes the one thing it measures. On
//! that plant the relative position of a node along the trunk is its height over 10.

use arbrume_core::curve::{Curve, OrderCurve};
use arbrume_core::graph::{Chain, NodeFlags, NodeId, PlantGraph};
use arbrume_core::species::{Branching, BranchingPattern, GrowthRhythm, Phyllotaxis, Species};
use arbrume_grow::{GrowParams, grow};
use glam::Vec3;

const TRUNK: f32 = 10.0;
const INTERNODE: f32 = 0.1;
/// Enough cycles for every axis of the plain plant to be full-grown.
const MATURE: u16 = 20;

fn plain() -> Species {
    let mut species = Species::default();
    species.architecture.max_order = 1;
    species.architecture.branching_pattern = BranchingPattern::Diffuse;
    species.foliage.enabled = false;
    let growth = &mut species.growth;
    growth.axis_length = OrderCurve::constants(&[TRUNK, 2.0]).unwrap();
    growth.shoot_length = OrderCurve::constants(&[1.0, 0.5]).unwrap();
    growth.internode_length = OrderCurve::constant(INTERNODE);
    growth.length_variation = OrderCurve::constant(0.0);
    growth.branch_probability = OrderCurve::constant(1.0);
    growth.branching_angle = OrderCurve::constant(90.0);
    growth.branching_angle_variation = OrderCurve::constant(0.0);
    growth.direction_variation = OrderCurve::constant(0.0);
    growth.tropism_weight = OrderCurve::constant(0.0);
    species
}

fn grown(species: &Species, seed: u64, age: u16) -> PlantGraph {
    species.validate().unwrap();
    let params = GrowParams {
        seed,
        age,
        scale: 1.0,
    };
    let graph = grow(species, &params);
    graph.check_invariants().unwrap();
    graph
}

fn points(points: &[[f32; 2]]) -> Curve {
    Curve::linear(points).unwrap()
}

fn laterals(graph: &PlantGraph) -> impl Iterator<Item = &Chain> {
    graph.chains().iter().filter(|chain| chain.order == 1)
}

/// Relative position of the attachment of `chain` along the plain trunk.
fn attachment(graph: &PlantGraph, chain: &Chain) -> f32 {
    graph.position(chain.attach).y / TRUNK
}

fn direction(graph: &PlantGraph, node: NodeId) -> Vec3 {
    (graph.position(node) - graph.proximal(node)).normalize()
}

fn angle_deg(a: Vec3, b: Vec3) -> f32 {
    a.dot(b).clamp(-1.0, 1.0).acos().to_degrees()
}

/// Horizontal distance from the trunk to the tip of `chain`.
fn reach(graph: &PlantGraph, chain: &Chain) -> f32 {
    let tip = graph.position(chain.last);
    tip.x.hypot(tip.z)
}

#[test]
fn the_length_curve_of_order_one_draws_the_silhouette() {
    let mut species = plain();
    let cone = points(&[[0.0, 4.0], [1.0, 1.0]]);
    species.growth.axis_length = OrderCurve::by_order(vec![Curve::constant(TRUNK), cone]).unwrap();
    let graph = grown(&species, 5, MATURE);
    assert_eq!(laterals(&graph).count(), 100);
    for chain in laterals(&graph) {
        let expected = 3.0f32.mul_add(-attachment(&graph, chain), 4.0);
        let tolerance = 0.5 * INTERNODE + 1e-3;
        assert!((reach(&graph, chain) - expected).abs() <= tolerance);
    }

    let top_heavy = points(&[[0.0, 1.0], [1.0, 4.0]]);
    species.growth.axis_length =
        OrderCurve::by_order(vec![Curve::constant(TRUNK), top_heavy]).unwrap();
    let inverted = grown(&species, 5, MATURE);
    let mean_reach = |graph: &PlantGraph, low: f32, high: f32| {
        let band = laterals(graph).filter(|chain| {
            let x = attachment(graph, chain);
            x >= low && x < high
        });
        let reaches: Vec<f32> = band.map(|chain| reach(graph, chain)).collect();
        reaches.iter().sum::<f32>() / reaches.len() as f32
    };
    assert!(mean_reach(&graph, 0.0, 0.33) > mean_reach(&graph, 0.67, 1.01) + 1.5);
    assert!(mean_reach(&inverted, 0.0, 0.33) + 1.5 < mean_reach(&inverted, 0.67, 1.01));
}

#[test]
fn apical_dominance_shortens_laterals_towards_the_tip() {
    let mut species = plain();
    species.growth.apical_dominance = 0.8;
    let graph = grown(&species, 5, MATURE);
    for chain in laterals(&graph) {
        let expected = 2.0 * 0.8f32.mul_add(-attachment(&graph, chain), 1.0);
        assert!((reach(&graph, chain) - expected).abs() <= 0.5 * INTERNODE + 1e-3);
    }
}

#[test]
fn the_parent_length_exponent_scales_laterals_with_their_parent() {
    let mut species = plain();
    species.architecture.max_order = 2;
    let cone = points(&[[0.0, 4.0], [1.0, 1.0]]);
    let curves = vec![Curve::constant(TRUNK), cone, Curve::constant(1.0)];
    species.growth.axis_length = OrderCurve::by_order(curves).unwrap();
    species.growth.parent_length_exponent = 1.0;
    let graph = grown(&species, 5, 30);
    let mut measured = 0;
    for chain in graph.chains().iter().filter(|chain| chain.order == 2) {
        let parent = graph.chain_of(chain.attach);
        let expected = graph.chain_length(parent) / 4.0;
        let length = graph.chain_length(graph.chain_of(chain.first));
        assert!((length - expected).abs() <= 0.5 * INTERNODE + 0.02);
        measured += 1;
    }
    assert!(measured > 1000);
}

#[test]
fn the_probability_curve_sets_where_and_how_often_laterals_form() {
    let mut species = plain();
    let curve = points(&[[0.0, 0.0], [0.4, 0.0], [0.6, 0.5], [1.0, 0.5]]);
    species.growth.branch_probability = OrderCurve::uniform(curve);
    let (mut formed, mut sites) = (0_u32, 0_u32);
    for seed in 0..30 {
        let graph = grown(&species, seed, MATURE);
        for chain in laterals(&graph) {
            let x = attachment(&graph, chain);
            assert!(x > 0.4);
            formed += u32::from(x > 0.6 + 1e-3);
        }
        sites += 39;
    }
    let frequency = formed as f32 / sites as f32;
    assert!((frequency - 0.5).abs() < 0.06, "frequency {frequency}");
}

#[test]
fn a_continuous_branching_pattern_develops_every_bud() {
    let mut species = plain();
    species.architecture.branching_pattern = BranchingPattern::Continuous;
    species.growth.branch_probability = OrderCurve::constant(0.05);
    assert_eq!(laterals(&grown(&species, 1, MATURE)).count(), 100);
}

#[test]
fn the_bare_base_follows_the_length_of_the_trunk_at_each_age() {
    let mut species = plain();
    species.growth.bare_base = OrderCurve::constant(0.3);
    for (age, height) in [(5, 5.0), (MATURE, TRUNK)] {
        let graph = grown(&species, 2, age);
        let lowest = laterals(&graph)
            .map(|chain| graph.position(chain.attach).y)
            .fold(f32::INFINITY, f32::min);
        let bare = 0.3 * height;
        assert!(lowest >= bare - 1e-3 && lowest <= bare + INTERNODE + 1e-3);
    }
}

#[test]
fn the_angle_curve_sets_the_insertion_angle() {
    let mut species = plain();
    let curve = points(&[[0.0, 80.0], [1.0, 30.0]]);
    species.growth.branching_angle = OrderCurve::uniform(curve);
    let graph = grown(&species, 4, MATURE);
    for chain in laterals(&graph) {
        let expected = 50.0f32.mul_add(-attachment(&graph, chain), 80.0);
        let angle = angle_deg(direction(&graph, chain.first), Vec3::Y);
        assert!((angle - expected).abs() < 0.05, "{angle} vs {expected}");
    }
}

#[test]
fn the_internode_curve_sets_the_node_density() {
    let mut species = plain();
    species.architecture.max_order = 0;
    let curve = points(&[[0.0, 0.2], [1.0, 0.05]]);
    species.growth.internode_length = OrderCurve::uniform(curve);
    let graph = grown(&species, 4, MATURE);
    let expected = TRUNK * (0.2_f32 / 0.05).ln() / 0.15;
    let count = graph.node_count() as f32;
    assert!(
        (count - expected).abs() < 3.0,
        "{count} nodes vs {expected}"
    );
    let lengths: Vec<f32> = graph
        .node_ids()
        .map(|node| graph.internode_length(node))
        .collect();
    assert!(lengths.windows(2).all(|pair| pair[1] <= pair[0] + 1e-4));
    assert!((graph.position(NodeId::new(count as usize - 1)).y - TRUNK).abs() < 0.03);
}

/// Horizontal directions of the laterals of the plain trunk, grouped by trunk node from
/// the base up.
fn azimuths_by_node(graph: &PlantGraph) -> Vec<Vec<Vec3>> {
    let mut by_node = vec![Vec::new(); 100];
    for chain in laterals(graph) {
        let node = (graph.position(chain.attach).y / INTERNODE).round() as usize - 1;
        by_node[node].push(direction(graph, chain.first));
    }
    by_node
}

#[test]
fn laterals_follow_the_phyllotaxis() {
    let cases = [
        (Phyllotaxis::spiral(), 137.5),
        (Phyllotaxis::distichous(), 180.0),
        (Phyllotaxis::decussate(), 90.0),
    ];
    for (phyllotaxis, between_nodes) in cases {
        let mut species = plain();
        species.architecture.phyllotaxis = phyllotaxis;
        let per_node = usize::from(phyllotaxis.leaves_per_node);
        let by_node = azimuths_by_node(&grown(&species, 6, MATURE));
        for node in &by_node {
            assert_eq!(node.len(), per_node);
            if let [first, second] = node[..] {
                assert!((angle_deg(first, second) - 180.0).abs() < 0.1);
            }
        }
        for pair in by_node.windows(2) {
            let turn = angle_deg(pair[0][0], pair[1][0]);
            assert!((turn - between_nodes).abs() < 0.1, "turn {turn}");
        }
    }
}

fn tiered() -> Species {
    let mut species = plain();
    species.architecture.branching_pattern = BranchingPattern::Rhythmic;
    species.growth.unit_branching = points(&[[0.0, 0.0], [0.95, 0.0], [1.0, 1.0]]);
    species.growth.whorl = OrderCurve::constant(5.0);
    species
}

#[test]
fn rhythmic_growth_gives_whorls_at_the_end_of_each_growth_unit() {
    let graph = grown(&tiered(), 8, MATURE);
    let by_node = azimuths_by_node(&graph);
    for (node, branches) in by_node.iter().enumerate() {
        let expected = if node % 10 == 9 { 5 } else { 0 };
        assert_eq!(branches.len(), expected, "trunk node {node}");
        for pair in branches.windows(2) {
            assert!((angle_deg(pair[0], pair[1]) - 72.0).abs() < 0.1);
        }
    }
    let starts = |flags: &NodeFlags| flags.contains(NodeFlags::GROWTH_UNIT_START);
    assert_eq!(
        graph.flags()[..100].iter().filter(|f| starts(f)).count(),
        10
    );
}

#[test]
fn continuous_growth_has_neither_growth_units_nor_whorls() {
    let mut species = tiered();
    species.architecture.growth_rhythm = GrowthRhythm::Continuous;
    let graph = grown(&species, 8, MATURE);
    assert!(graph.flags().iter().all(|flags| *flags == NodeFlags::NONE));
    let by_node = azimuths_by_node(&graph);
    assert!(by_node.iter().all(|branches| branches.len() == 1));
}

#[test]
fn immediate_branching_starts_laterals_one_cycle_earlier() {
    let mut species = plain();
    let delayed = grown(&species, 3, 1);
    assert_eq!(delayed.chain_count(), 1);
    species.architecture.branch_timing = arbrume_core::species::BranchTiming::Sylleptic;
    let immediate = grown(&species, 3, 1);
    assert_eq!(immediate.chain_count(), 11);
}

#[test]
fn forks_double_the_stems_and_open_by_twice_the_fork_angle() {
    let mut species = plain();
    species.architecture.max_order = 0;
    species.growth.fork_probability = OrderCurve::constant(1.0);
    species.growth.fork_angle = OrderCurve::constant(20.0);
    let graph = grown(&species, 9, 4);
    assert_eq!(graph.chain_count(), 8);
    assert!(graph.orders().iter().all(|&order| order == 0));
    let children = graph.children();
    let mut forks = 0;
    for node in graph.node_ids() {
        if let &[first, second] = children.of(node) {
            let opening = angle_deg(direction(&graph, first), direction(&graph, second));
            assert!((opening - 40.0).abs() < 0.1, "opening {opening}");
            forks += 1;
        }
    }
    assert_eq!(forks, 7);
}

#[test]
fn the_fork_limit_caps_successive_forks() {
    let mut species = plain();
    species.architecture.max_order = 0;
    species.growth.fork_probability = OrderCurve::constant(1.0);
    for (limit, stems) in [(0, 1), (1, 2), (2, 4)] {
        species.growth.fork_limit = limit;
        assert_eq!(grown(&species, 9, 8).chain_count(), stems, "limit {limit}");
    }
}

#[test]
fn laterals_older_than_their_lifespan_are_shed_and_droop_with_age() {
    let mut species = plain();
    species.growth.branch_lifespan = OrderCurve::constant(3.0);
    species.growth.branching_angle = OrderCurve::constant(30.0);
    species.growth.aging_angle = points(&[[0.0, 0.0], [1.0, 60.0]]);
    let graph = grown(&species, 2, 8);
    let trunk_top = graph.position(graph.chains()[0].last).y;
    assert!((trunk_top - 8.0).abs() < 1e-3);
    assert_eq!(laterals(&graph).count(), 20);
    for chain in laterals(&graph) {
        let height = graph.position(chain.attach).y;
        assert!(
            height > 5.0 - 1e-3,
            "lateral at {height} outlived its lifespan"
        );
        let cycle = (height - 1e-3).ceil() - 1.0;
        let relative_age = (7.0 - cycle) / 2.0;
        let angle = angle_deg(direction(&graph, chain.first), Vec3::Y);
        let expected = 30.0 + 60.0 * relative_age;
        assert!((angle - expected).abs() < 0.5, "cycle {cycle}: {angle}");
    }
    species.growth.branch_lifespan = OrderCurve::constant(0.0);
    assert_eq!(laterals(&grown(&species, 2, 8)).count(), 70);
}

#[test]
fn foliage_density_order_and_count_per_node_select_the_leaf_sites() {
    let mut species = plain();
    species.foliage.enabled = true;
    species.foliage.retention_cycles = u16::MAX;
    let full = grown(&species, 6, MATURE);
    assert_eq!(full.organs().len(), full.node_count());
    species.foliage.per_node = 3;
    let tripled = grown(&species, 6, MATURE);
    assert_eq!(tripled.organs().len(), 3 * full.node_count());
    let node = tripled.organs()[0].node;
    let around: Vec<Vec3> = tripled
        .organs()
        .iter()
        .filter(|site| site.node == node)
        .map(|site| site.direction)
        .collect();
    assert_eq!(around.len(), 3);
    assert!(around[0].dot(around[1]) < 0.9);
    species.foliage.per_node = 0;
    species.foliage.min_order = 1;
    let branches_only = grown(&species, 6, MATURE);
    assert_eq!(branches_only.organs().len(), full.node_count() - 100);
    let orders = branches_only.orders();
    let sites = branches_only.organs().iter();
    assert!(sites.clone().all(|site| orders[site.node.index()] == 1));
    species.foliage.min_order = 0;
    species.foliage.density = 0.3;
    let thinned = grown(&species, 6, MATURE);
    let kept = thinned.organs().len() as f32 / full.organs().len() as f32;
    assert!((kept - 0.3).abs() < 0.05, "kept {kept}");
    assert_eq!(thinned.organs(), grown(&species, 6, MATURE).organs());
    let younger = grown(&species, 6, 6);
    assert!(younger.organs().len() < thinned.organs().len());
}

#[test]
fn a_sympodial_stem_is_a_succession_of_relays() {
    let mut species = plain();
    species.architecture.max_order = 0;
    species.architecture.branching = Branching::Sympodial;
    species.growth.relay_angle = OrderCurve::constant(15.0);
    let graph = grown(&species, 9, 6);
    assert_eq!(graph.chain_count(), 6);
    assert_eq!(graph.node_count(), 60);
    for chain in &graph.chains()[1..] {
        assert_eq!(chain.order, 0);
        let turn = angle_deg(
            direction(&graph, chain.first),
            direction(&graph, chain.attach),
        );
        assert!((turn - 15.0).abs() < 0.1);
    }
    let full = grown(&species, 9, MATURE);
    assert_eq!((full.chain_count(), full.node_count()), (10, 100));
}

#[test]
fn curvature_and_tropism_bend_the_axes() {
    let mut species = plain();
    species.growth.curvature = OrderCurve::constants(&[0.0, -90.0]).unwrap();
    let curved = grown(&species, 2, MATURE);
    for chain in laterals(&curved) {
        assert!(angle_deg(direction(&curved, chain.last), Vec3::Y) < 0.5);
    }

    let mut species = plain();
    species.growth.gravitropic_setpoint = OrderCurve::constant(0.0);
    species.growth.tropism_weight = OrderCurve::constants(&[0.0, 0.2]).unwrap();
    let rising = grown(&species, 2, MATURE);
    for chain in laterals(&rising) {
        let range = rising.chain_range(rising.chain_of(chain.first)).unwrap();
        let slopes: Vec<f32> = range
            .map(|node| direction(&rising, NodeId::new(node)).y)
            .collect();
        assert!(slopes.windows(2).all(|pair| pair[1] > pair[0]));
        assert!(slopes[slopes.len() - 1] > 0.95);
    }
}

#[test]
fn weight_bending_lowers_the_branch_tips() {
    let mut species = plain();
    let straight = grown(&species, 2, MATURE);
    species.growth.weight_bending = 1e-6;
    let bent = grown(&species, 2, MATURE);
    assert_eq!(straight.node_count(), bent.node_count());
    let chains = laterals(&straight).zip(laterals(&bent));
    for (before, after) in chains {
        assert!(bent.position(after.last).y < straight.position(before.last).y);
    }
}

#[test]
fn leaves_sit_on_the_youngest_growth_at_the_inclination_of_the_species() {
    let mut species = plain();
    species.foliage.enabled = true;
    species.foliage.retention_cycles = 1;
    species.foliage.inclination_deg = 40.0;

    let young = grown(&species, 3, 3);
    assert!(young.organs().iter().all(|site| site.birth == 2));
    let last_cycle = young.births().iter().filter(|&&birth| birth == 2).count();
    assert_eq!(young.organs().len(), last_cycle);
    for site in young.organs() {
        let inclination = angle_deg(site.direction, direction(&young, site.node));
        assert!((inclination - 40.0).abs() < 0.1);
        assert!(site.direction.dot(site.normal).abs() < 1e-4);
    }

    let mature = grown(&species, 3, 40);
    let leafy: Vec<NodeId> = mature.organs().iter().map(|site| site.node).collect();
    assert_eq!(leafy.len(), 10 + 100 * 5);
    for chain in mature.chains() {
        let shoot = if chain.order == 0 { 10 } else { 5 };
        let tip = chain.last.index();
        let on_chain = leafy
            .iter()
            .filter(|node| mature.chain_of(**node) == mature.chain_of(chain.last));
        assert!(on_chain.clone().all(|node| node.index() + shoot > tip));
        assert_eq!(on_chain.count(), shoot);
    }

    species.foliage.retention_cycles = 3;
    assert_eq!(grown(&species, 3, 40).organs().len(), 3 * (10 + 100 * 5));
}
