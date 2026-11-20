//! Root systems: structure, determinism, the ground, obstacles, resources and the pipe
//! model at the collar.

mod common;

use arbrume_core::graph::{NodeId, PlantGraph};
use arbrume_core::species::{GrowthMode, RootHabit, Roots, Species};
use arbrume_grow::{
    Ground, GrowParams, HeightField, Occluder, Resource, Shape, Soil, Surroundings, grow_in,
    grow_rooted,
};
use common::starter;
// The fixture list of the other test files.
const _: [&str; 3] = common::STARTERS;
use glam::Vec3;

/// The broadleaf fixture with the roots of `habit` in `mode`.
fn rooted(habit: RootHabit, mode: GrowthMode) -> Species {
    let mut species = starter("broadleaf");
    let mut roots = Roots::of(habit);
    roots.mode = mode;
    species.roots = Some(roots);
    species.validate().unwrap();
    species
}

const MODES: [GrowthMode; 2] = [GrowthMode::Parametric, GrowthMode::SelfOrganizing];

fn params(seed: u64, age: u16) -> GrowParams {
    GrowParams {
        seed,
        age,
        scale: 1.0,
    }
}

/// The root nodes of `graph` that are meshed (not shed).
fn root_nodes(graph: &PlantGraph) -> impl Iterator<Item = NodeId> + '_ {
    graph
        .node_ids()
        .filter(|node| graph.flags()[node.index()].is_root() && !graph.is_shed(*node))
}

fn grown(species: &Species, seed: u64, surroundings: &Surroundings, soil: &Soil) -> PlantGraph {
    grow_rooted(species, &params(seed, 14), surroundings, soil)
}

#[test]
fn every_habit_grows_a_valid_root_system_below_the_collar() {
    for habit in RootHabit::ALL {
        for mode in MODES {
            let species = rooted(habit, mode);
            let graph = grown(&species, 3, &Surroundings::none(), &Soil::homogeneous());
            graph.check_invariants().unwrap();
            assert!(graph.is_contiguous(), "{habit:?} {mode:?}");
            let roots = root_nodes(&graph).count();
            assert!(roots > 20, "{habit:?} {mode:?}: {roots} root nodes");
            let trunk = graph.chain(arbrume_core::graph::ChainId(0));
            assert!(graph.positions()[trunk.first.index()].y.abs() < 1e-6);
            for chain in graph.chains() {
                let root = graph.flags()[chain.first.index()].is_root();
                if root && chain.attach.is_some() {
                    let parent = chain.attach;
                    let parent_root = graph.flags()[parent.index()].is_root();
                    assert!(parent_root || parent == trunk.first, "{habit:?}");
                }
            }
        }
    }
}

#[test]
fn a_species_without_roots_grows_exactly_as_before() {
    let species = starter("broadleaf");
    for seed in [1, 2] {
        let soil = Soil::homogeneous();
        let rooted = grow_rooted(&species, &params(seed, 12), &Surroundings::none(), &soil);
        assert_eq!(
            rooted,
            grow_in(&species, &params(seed, 12), &Surroundings::none())
        );
    }
}

#[test]
fn roots_are_deterministic_and_independent_of_threads() {
    for mode in MODES {
        let species = rooted(RootHabit::Heart, mode);
        let run = |threads: usize| {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap();
            pool.install(|| grown(&species, 7, &Surroundings::none(), &Soil::homogeneous()))
        };
        let one = run(1);
        assert_eq!(one, run(4), "{mode:?}");
        assert_eq!(one, run(1), "{mode:?}");
    }
}

#[test]
fn buried_roots_stay_under_uneven_ground() {
    let heights = (0..11 * 11)
        .map(|index| 0.25 * ((index % 11) as f32 * 0.7).sin() + 0.1 * (index / 11) as f32)
        .collect();
    let field = HeightField {
        origin: [-5.0, -5.0],
        cell: 1.0,
        size: [11, 11],
        heights,
    };
    let grounds = [
        Ground::Flat,
        Ground::Slope { rise: [0.3, -0.1] },
        Ground::Heights(field),
    ];
    for ground in grounds {
        let soil = Soil {
            ground,
            resources: Vec::new(),
        };
        for mode in MODES {
            let graph = grown(
                &rooted(RootHabit::Plate, mode),
                5,
                &Surroundings::none(),
                &soil,
            );
            for node in root_nodes(&graph) {
                let point = graph.positions()[node.index()];
                let top = point.y + graph.radii()[node.index()];
                let surface = soil.ground.height(point.x, point.z);
                assert!(top <= surface + 1e-4, "{mode:?} {point} under {surface}");
            }
        }
    }
}

#[test]
fn exposed_roots_show_near_the_collar_only() {
    let mut species = rooted(RootHabit::Plate, GrowthMode::Parametric);
    let roots = species.roots.as_mut().unwrap();
    roots.exposed = 0.6;
    let reach = roots.exposed_length;
    let graph = grown(&species, 2, &Surroundings::none(), &Soil::homogeneous());
    let mut shown = 0;
    for node in root_nodes(&graph) {
        let point = graph.positions()[node.index()];
        let top = point.y + graph.radii()[node.index()];
        if top > 1e-4 {
            shown += 1;
            let across = Vec3::new(point.x, 0.0, point.z).length();
            assert!(across < reach, "exposed root at {point}");
        }
    }
    assert!(shown > 3, "{shown} exposed root nodes");
}

#[test]
fn roots_go_around_a_rock() {
    let rock = Shape::Sphere {
        center: Vec3::new(0.0, -1.4, 0.0),
        radius: 0.9,
    };
    let surroundings = Surroundings {
        envelope: None,
        occluders: vec![Occluder::obstacle(rock.clone())],
        water: None,
    };
    for mode in MODES {
        let species = rooted(RootHabit::Tap, mode);
        let soil = Soil::homogeneous();
        let open = grown(&species, 4, &Surroundings::none(), &soil);
        let blocked = |graph: &PlantGraph| {
            root_nodes(graph)
                .filter(|node| rock.contains(graph.positions()[node.index()]))
                .count()
        };
        assert!(
            blocked(&open) > 0,
            "{mode:?}: the tap root does not reach the rock"
        );
        let around = grown(&species, 4, &surroundings, &soil);
        assert_eq!(blocked(&around), 0, "{mode:?}");
        let deep = root_nodes(&around)
            .map(|node| around.positions()[node.index()].y)
            .fold(0.0, f32::min);
        assert!(deep < -1.6, "{mode:?}: the roots stop at the rock ({deep})");
    }
}

#[test]
fn a_resource_patch_attracts_the_roots() {
    let patch = Resource {
        shape: Shape::Sphere {
            center: Vec3::new(2.5, -0.8, 0.0),
            radius: 0.8,
        },
        richness: 6.0,
        reach: 2.0,
    };
    let rich = Soil {
        ground: Ground::Flat,
        resources: vec![patch.clone()],
    };
    for mode in MODES {
        let species = rooted(RootHabit::Heart, mode);
        let near = |graph: &PlantGraph| {
            root_nodes(graph)
                .filter(|node| patch.shape.distance(graph.positions()[node.index()]) < 0.5)
                .count()
        };
        let (mut plain_near, mut fed_near) = (0, 0);
        for seed in [1, 2, 3] {
            let plain = grown(&species, seed, &Surroundings::none(), &Soil::homogeneous());
            let fed = grown(&species, seed, &Surroundings::none(), &rich);
            plain_near += near(&plain);
            fed_near += near(&fed);
        }
        let summary = format!("{mode:?}: {fed_near} root nodes near the patch, {plain_near}");
        assert!(fed_near > plain_near + 6, "{summary} without it");
    }
}

#[test]
fn the_trunk_base_carries_the_pipes_of_shoot_and_roots() {
    let species = rooted(RootHabit::Heart, GrowthMode::Parametric);
    let shoot = grow_in(&species, &params(6, 14), &Surroundings::none());
    let graph = grown(&species, 6, &Surroundings::none(), &Soil::homogeneous());
    let collar = graph.chain(arbrume_core::graph::ChainId(0)).first;
    let roots = species.roots.as_ref().unwrap();
    assert!((roots.pipe_balance - 1.0).abs() < f32::EPSILON);
    let n = roots.pipe_exponent;
    let stem = graph.radii()[collar.index()].powf(n);
    let alone = shoot.radii()[collar.index()].powf(n);
    assert!((stem / alone - 2.0).abs() < 1e-3, "{stem} {alone}");
}
