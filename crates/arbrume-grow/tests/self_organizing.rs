//! The self-organizing mode: structural properties and determinism over random seeds,
//! the relation between a plant and the same plant grown longer, and the response of a
//! plant to what stands around it.

mod common;

use arbrume_core::graph::PlantGraph;
use arbrume_core::species::{Allocation, Sensing, Species};
use arbrume_grow::{
    Context, EngineOptions, GridStorage, GrowParams, Occluder, Shape, Standing, Surroundings,
    VoxelMask, default_envelope, grow, grow_in, grow_in_with, grow_timeline, grow_together,
    radii_at_cycle, run_cycle, seed_plant, self_organizing_stages,
};
use common::starter;
// The fixture list of the other test files.
const _: [&str; 3] = common::STARTERS;
use glam::Vec3;
use proptest::prelude::*;
use std::sync::Arc;

/// The fixture with the given sensing and allocation.
fn competing(sensing: Sensing, allocation: Allocation) -> Species {
    let mut species = starter("competing");
    species.growth.self_organization.sensing = sensing;
    species.growth.self_organization.allocation = allocation;
    if sensing != Sensing::Shadow {
        species.growth.self_organization.shedding_threshold = 0.0;
    }
    species.validate().unwrap();
    species
}

/// The fixture in its default form: shadow propagation and Borchert-Honda.
fn fixture() -> Species {
    competing(Sensing::Shadow, Allocation::BorchertHonda)
}

const SENSINGS: [Sensing; 3] = [Sensing::Shadow, Sensing::Markers, Sensing::Both];
const ALLOCATIONS: [Allocation; 2] = [Allocation::BorchertHonda, Allocation::Priority];

fn params(seed: u64, age: u16) -> GrowParams {
    GrowParams {
        seed,
        age,
        scale: 1.0,
    }
}

/// Every stored number of a graph as bits, for bit-exact comparison.
fn fingerprint(graph: &PlantGraph) -> Vec<u32> {
    let positions = graph.positions().iter().flat_map(Vec3::to_array);
    let mut bits: Vec<u32> = positions.map(f32::to_bits).collect();
    bits.extend(graph.radii().iter().map(|r| r.to_bits()));
    bits.extend(graph.parents().iter().map(|parent| parent.0));
    bits.extend(graph.births().iter().map(|&birth| u32::from(birth)));
    bits.extend(graph.flags().iter().map(|flags| u32::from(flags.0)));
    bits.extend(graph.shed_cycles().iter().map(|&cycle| u32::from(cycle)));
    for site in graph.organs() {
        bits.push(site.node.0);
        bits.extend(site.direction.to_array().map(f32::to_bits));
        bits.extend(site.normal.to_array().map(f32::to_bits));
    }
    bits
}

/// Mean position of the live nodes.
fn centroid(graph: &PlantGraph) -> Vec3 {
    let live = || {
        let nodes = graph.node_ids().filter(|&node| !graph.is_shed(node));
        nodes.map(|node| graph.position(node))
    };
    live().sum::<Vec3>() / live().count().max(1) as f32
}

/// Births and position bits of the nodes born before cycle `age`, sorted.
fn born_before(graph: &PlantGraph, age: u16) -> Vec<(u16, [u32; 3])> {
    let nodes = graph.births().iter().zip(graph.positions());
    let mut nodes: Vec<(u16, [u32; 3])> = nodes
        .filter(|(birth, _)| **birth < age)
        .map(|(&birth, position)| (birth, position.to_array().map(f32::to_bits)))
        .collect();
    nodes.sort_unstable();
    nodes
}

fn pool(threads: usize) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn grown_plants_are_well_formed(
        seed in any::<u64>(),
        age in 0_u16..12,
        sensing in 0_usize..3,
        allocation in 0_usize..2,
        scale in 0.5_f32..2.0,
    ) {
        let species = competing(SENSINGS[sensing], ALLOCATIONS[allocation]);
        let graph = grow(&species, &GrowParams { seed, age, scale });
        prop_assert_eq!(graph.check_invariants(), Ok(()));
        prop_assert!(graph.is_contiguous());
        prop_assert_eq!(graph.node_count() == 0, age == 0);
        prop_assert!(graph.radii().iter().all(|&r| r.is_finite() && r > 0.0));
        prop_assert!(graph.positions().iter().all(|p| p.is_finite() && p.y >= 0.0));
        for site in graph.organs() {
            prop_assert!((site.direction.length() - 1.0).abs() < 1e-3);
            prop_assert!(site.direction.dot(site.normal).abs() < 1e-3);
        }
        let shed = graph.flags().iter().zip(graph.shed_cycles());
        prop_assert!(shed.filter(|(flags, _)| flags.is_shed()).all(|(_, &cycle)| cycle < age));
    }

    #[test]
    fn reruns_and_engine_options_give_the_same_bits(
        seed in any::<u64>(),
        age in 4_u16..11,
        sensing in 0_usize..3,
        allocation in 0_usize..2,
    ) {
        let species = competing(SENSINGS[sensing], ALLOCATIONS[allocation]);
        let params = params(seed, age);
        let open = Surroundings::none();
        let reference = fingerprint(&grow(&species, &params));
        prop_assert_eq!(&fingerprint(&grow(&species, &params)), &reference);
        let option = |storage, parallel| EngineOptions {
            storage,
            parallel_buds: parallel,
            parallel_markers: parallel,
        };
        let sequential = option(GridStorage::Sparse, false);
        let grown = grow_in_with(&species, &params, &open, sequential);
        prop_assert_eq!(&fingerprint(&grown), &reference);
        let parallel = option(GridStorage::Dense, true);
        for threads in [1, 2, 8] {
            let grown = pool(threads).install(|| grow_in_with(&species, &params, &open, parallel));
            prop_assert_eq!(&fingerprint(&grown), &reference, "{} threads", threads);
            let grown = pool(threads).install(|| grow(&species, &params));
            prop_assert_eq!(&fingerprint(&grown), &reference, "{} threads", threads);
        }
    }

    #[test]
    fn a_young_plant_is_the_start_of_an_older_one(
        seed in any::<u64>(),
        young in 1_u16..8,
        more in 1_u16..5,
        sensing in 0_usize..3,
        allocation in 0_usize..2,
    ) {
        let species = competing(SENSINGS[sensing], ALLOCATIONS[allocation]);
        let early = grow(&species, &params(seed, young));
        let late = grow(&species, &params(seed, young + more));
        prop_assert_eq!(born_before(&early, young), born_before(&late, young));
        prop_assert!(early.chain_count() <= late.chain_count());
        let live = |graph: &PlantGraph| graph.flags().iter().filter(|f| !f.is_shed()).count();
        let still = late.births().iter().zip(late.flags()).zip(late.shed_cycles());
        let then = still.filter(|((birth, flags), shed)| {
            **birth < young && (!flags.is_shed() || **shed >= young)
        });
        prop_assert_eq!(live(&early), then.count());
    }
}

#[test]
fn the_resource_of_a_growing_plant_is_conserved() {
    for allocation in ALLOCATIONS {
        let species = competing(Sensing::Shadow, allocation);
        let grow_params = params(11, 9);
        let cx: Context = grow_params.context(&species);
        let open = Surroundings::none();
        let mut stages = self_organizing_stages(&cx, &open, EngineOptions::default());
        let mut plant = seed_plant(&cx, &mut stages.fate);
        for cycle in 0..cx.age {
            run_cycle(&mut plant, &cx, &mut stages, cycle);
            let alpha = species.growth.self_organization.resource_coefficient;
            let gathered = stages.fate.gathered();
            let stems = plant.meristems.iter().zip(gathered);
            let base: f32 = stems
                .filter(|(meristem, _)| meristem.parent.is_none())
                .map(|(_, light)| alpha * light)
                .sum();
            let given: f32 = stages.fate.resources().iter().sum();
            assert!(
                (given - base).abs() <= 1e-3 * base.max(1.0),
                "{cycle}: {given} {base}"
            );
        }
        assert!(plant.graph.node_count() > 30);
    }
}

#[test]
fn a_larger_axis_bias_makes_a_taller_narrower_tree() {
    let mut slender = 0;
    for seed in 0..6 {
        let shape = |bias: f32| {
            let mut species = fixture();
            species.growth.self_organization.axis_bias =
                arbrume_core::curve::OrderCurve::constant(bias);
            species.growth.self_organization.shedding_threshold = 0.0;
            let size = grow(&species, &params(seed, 11)).bounds().unwrap().size();
            size.y / size.x.max(size.z)
        };
        slender += usize::from(shape(0.58) > shape(0.44));
    }
    assert!(slender >= 5, "{slender} of 6");
}

#[test]
fn a_tree_grows_away_from_a_wall() {
    let species = fixture();
    let wall = Shape::Box {
        min: Vec3::new(1.0, 0.0, -10.0),
        max: Vec3::new(1.5, 12.0, 10.0),
    };
    let beside = Surroundings {
        envelope: None,
        occluders: vec![Occluder::obstacle(wall.clone())],
    };
    let mut shift = 0.0;
    for seed in 0..4 {
        let open = grow(&species, &params(seed, 12));
        let walled = grow_in(&species, &params(seed, 12), &beside);
        walled.check_invariants().unwrap();
        assert!(walled.positions().iter().all(|&p| !wall.contains(p)));
        assert!(walled.positions().iter().all(|p| p.x < 1.0 + 1e-3));
        assert!(open.positions().iter().any(|p| p.x > 1.5));
        shift += centroid(&open).x - centroid(&walled).x;
    }
    assert!(shift / 4.0 > 0.2, "mean shift {}", shift / 4.0);
}

#[test]
fn shaded_lower_branches_are_shed() {
    let species = fixture();
    let mut kept = fixture();
    kept.growth.self_organization.shedding_threshold = 0.0;
    let graph = grow(&species, &params(3, 14));
    let whole = grow(&kept, &params(3, 14));
    assert!(whole.flags().iter().all(|flags| !flags.is_shed()));
    let heights = |shed: bool| -> Vec<f32> {
        let nodes = graph.node_ids().filter(|&node| graph.is_shed(node) == shed);
        nodes.map(|node| graph.position(node).y).collect()
    };
    let mean = |values: &[f32]| values.iter().sum::<f32>() / values.len().max(1) as f32;
    let (shed, live) = (heights(true), heights(false));
    assert!(shed.len() > 20, "{} shed nodes", shed.len());
    assert!(mean(&shed) < mean(&live), "{} {}", mean(&shed), mean(&live));
    // The trunk is never shed.
    let trunk = graph.chain_view(graph.chain_ids().next().unwrap()).unwrap();
    assert_eq!(trunk.len(), graph.chains()[0].len as usize);
    // A dense crown overhead takes more branches.
    let canopy = Shape::Box {
        min: Vec3::new(-20.0, 5.0, -20.0),
        max: Vec3::new(20.0, 6.0, 20.0),
    };
    let under = Surroundings {
        envelope: None,
        occluders: vec![Occluder::neighbour(canopy, 3.0)],
    };
    let shaded = grow_in(&species, &params(3, 14), &under);
    let live_count = |graph: &PlantGraph| graph.flags().iter().filter(|f| !f.is_shed()).count();
    assert!(live_count(&shaded) < live_count(&graph) / 2);
}

/// Largest distance by which a node of `graph` lies outside `envelope`.
fn overshoot(graph: &PlantGraph, envelope: &Shape) -> f32 {
    let outside = graph.positions().iter().map(|&p| envelope.distance(p));
    outside.fold(0.0, f32::max)
}

#[test]
fn an_envelope_is_respected() {
    let ellipsoid = Shape::Ellipsoid {
        center: Vec3::new(0.0, 2.5, 0.0),
        radii: Vec3::new(1.5, 2.5, 2.0),
    };
    let cuboid = Shape::Box {
        min: Vec3::new(-1.0, 0.0, -2.0),
        max: Vec3::new(1.0, 3.0, 2.0),
    };
    let ball = |p: Vec3| p.distance(Vec3::new(0.0, 2.0, 0.0)) < 2.0;
    let mask = VoxelMask::from_fn(Vec3::new(-2.0, 0.0, -2.0), 0.1, [40, 40, 40], ball);
    let voxels = Shape::Voxels(Arc::new(mask));
    for sensing in SENSINGS {
        let species = competing(sensing, Allocation::BorchertHonda);
        for envelope in [&ellipsoid, &cuboid] {
            let surroundings = Surroundings {
                envelope: Some(envelope.clone()),
                occluders: Vec::new(),
            };
            let graph = grow_in(&species, &params(5, 12), &surroundings);
            graph.check_invariants().unwrap();
            assert!(
                graph.node_count() > 60,
                "{sensing:?}: {}",
                graph.node_count()
            );
            let slack = overshoot(&graph, envelope);
            assert!(slack <= 1e-4, "{sensing:?} {envelope:?}: {slack}");
            let open = grow(&species, &params(5, 12));
            assert!(overshoot(&open, envelope) > 1.0);
        }
    }
    let surroundings = Surroundings {
        envelope: Some(voxels.clone()),
        occluders: Vec::new(),
    };
    let graph = grow_in(&fixture(), &params(5, 12), &surroundings);
    let centre = Vec3::new(0.0, 2.0, 0.0);
    let reach = graph.positions().iter().map(|p| p.distance(centre));
    assert!(reach.fold(0.0, f32::max) < 2.25 && graph.node_count() > 60);
}

#[test]
fn markers_keep_a_crown_inside_its_default_envelope() {
    let species = competing(Sensing::Markers, Allocation::BorchertHonda);
    let envelope = default_envelope(&species, 1.0);
    let graph = grow(&species, &params(2, 14));
    assert!(graph.node_count() > 100);
    let unit = species.growth.internode_unit();
    let reach = species.growth.self_organization.perception_distance * unit;
    assert!(overshoot(&graph, &envelope) < reach);
}

#[test]
fn two_trees_side_by_side_lean_apart() {
    let species = fixture();
    let open = Surroundings::none();
    let stand = |seed: u64, x: f32| Standing {
        species: &species,
        params: params(seed, 12),
        position: Vec3::new(x, 0.0, 0.0),
    };
    let mut apart = 0.0;
    for seed in 0..3 {
        let pair = grow_together(&[stand(seed, -1.0), stand(seed + 10, 1.0)], &open);
        assert_eq!(pair.len(), 2);
        for graph in &pair {
            graph.check_invariants().unwrap();
        }
        let alone = [
            grow(&species, &params(seed, 12)),
            grow(&species, &params(seed + 10, 12)),
        ];
        apart += centroid(&alone[0]).x - centroid(&pair[0]).x;
        apart += centroid(&pair[1]).x - centroid(&alone[1]).x;
        let again = grow_together(&[stand(seed, -1.0), stand(seed + 10, 1.0)], &open);
        assert_eq!(fingerprint(&again[0]), fingerprint(&pair[0]));
    }
    assert!(apart / 6.0 > 0.1, "mean shift {}", apart / 6.0);
    // A single plant of a group is the plant grown alone.
    let single = grow_together(&[stand(4, 0.0)], &open);
    assert_eq!(
        fingerprint(&single[0]),
        fingerprint(&grow(&species, &params(4, 12)))
    );
}

#[test]
fn a_grown_plant_can_be_shown_at_earlier_ages() {
    let species = fixture();
    let open = Surroundings::none();
    let age = 13;
    let record = grow_timeline(&species, &params(6, age), &open);
    let grown = grow(&species, &params(6, age));
    assert_eq!(record.graph.positions(), grown.positions());
    assert_eq!(record.graph.radii(), grown.radii());
    assert!(record.graph.organs().len() > grown.organs().len());
    let mut radii = Vec::new();
    radii_at_cycle(&grown, &species, 1.0, age - 1, &mut radii);
    assert_eq!(radii, grown.radii());
    let mut trunk = f32::MAX;
    for cycle in (2..age).rev() {
        let past = record.at_cycle(&species, 1.0, cycle);
        past.check_invariants().unwrap();
        let younger = grow(&species, &params(6, cycle + 1));
        let live = |graph: &PlantGraph| {
            let nodes = graph.births().iter().zip(graph.flags());
            nodes
                .filter(|(birth, flags)| **birth <= cycle && !flags.is_shed())
                .count()
        };
        assert_eq!(live(&past), live(&younger), "cycle {cycle}");
        let births = past.births().iter().zip(past.radii());
        assert!(
            births
                .clone()
                .all(|(birth, radius)| (*birth > cycle) == (*radius == 0.0))
        );
        let mut then: Vec<f32> = past.radii().iter().copied().filter(|r| *r > 0.0).collect();
        let mut expected = younger.radii().to_vec();
        then.sort_by(f32::total_cmp);
        expected.sort_by(f32::total_cmp);
        assert_eq!(then.len(), expected.len());
        let close = |(a, b): (&f32, &f32)| (a - b).abs() <= 1e-4 * b.abs();
        assert!(then.iter().zip(&expected).all(close), "cycle {cycle}");
        assert!(past.radii()[0] < trunk);
        trunk = past.radii()[0];
    }
}

#[test]
fn parametric_species_ignore_their_surroundings() {
    let species = starter("broadleaf");
    let wall = Shape::Box {
        min: Vec3::new(-5.0, 0.0, -5.0),
        max: Vec3::new(5.0, 20.0, 5.0),
    };
    let inside = Surroundings {
        envelope: None,
        occluders: vec![Occluder::obstacle(wall)],
    };
    let params = params(1, 8);
    let plain = grow(&species, &params);
    assert_eq!(
        fingerprint(&grow_in(&species, &params, &inside)),
        fingerprint(&plain)
    );
}

/// Horizontal offset along `-x` of the centroid of the live nodes of `graph`.
fn lean_towards_minus_x(graph: &PlantGraph) -> f32 {
    let live = graph.node_ids().filter(|&node| !graph.is_shed(node));
    let xs: Vec<f32> = live.map(|node| graph.position(node).x).collect();
    -xs.iter().sum::<f32>() / xs.len().max(1) as f32
}

/// Offsets of [`a_crown_leans_away_from_a_shading_wall`], seeds 1 to 3, for buds that
/// take the shade of the wall into their exposure only, not into their direction. Turning
/// the buds away from the shade must lean the crown further.
const LEAN_FROM_EXPOSURE_ALONE: [f32; 3] = [0.965_432_05, 0.793_530_2, 1.147_880_6];

#[test]
fn a_crown_leans_away_from_a_shading_wall() {
    let species = fixture();
    let wall = Shape::Box {
        min: Vec3::new(1.0, 0.0, -4.0),
        max: Vec3::new(2.0, 10.0, 4.0),
    };
    let beside = Surroundings {
        envelope: None,
        occluders: vec![Occluder::neighbour(wall, 2.0)],
    };
    for (seed, before) in (1..=3).zip(LEAN_FROM_EXPOSURE_ALONE) {
        let open = lean_towards_minus_x(&grow(&species, &params(seed, 14)));
        let lean = lean_towards_minus_x(&grow_in(&species, &params(seed, 14), &beside));
        assert!(
            lean > open,
            "seed {seed}: {lean} beside the wall, {open} in the open"
        );
        assert!(lean > before, "seed {seed}: {lean} is not beyond {before}");
    }
}

/// Nodes of `graph` inside `shape`, and the largest height of a node.
fn inside_and_height(graph: &PlantGraph, shape: &Shape) -> (usize, f32) {
    let inside = graph.positions().iter().filter(|&&p| shape.contains(p));
    let height = graph.positions().iter().map(|p| p.y).fold(0.0, f32::max);
    (inside.count(), height)
}

/// A sphere floating above the ground, as an analytic shape and as a voxel mask whose
/// grid reaches down to the ground: with the shadow model a trunk rises into both and
/// fills them alike.
#[test]
fn a_trunk_rises_into_a_floating_voxel_envelope_as_into_an_analytic_one() {
    let (centre, radius) = (Vec3::new(0.0, 4.0, 0.0), 1.5);
    let sphere = Shape::Sphere {
        center: centre,
        radius,
    };
    let ball = |p: Vec3| p.distance(centre) < radius;
    let mask = VoxelMask::from_fn(Vec3::new(-2.0, 0.0, -2.0), 0.1, [40, 60, 40], ball);
    let voxels = Shape::Voxels(Arc::new(mask));
    let mut grown = Vec::new();
    for envelope in [&sphere, &voxels] {
        let surroundings = Surroundings {
            envelope: Some(envelope.clone()),
            occluders: Vec::new(),
        };
        let graph = grow_in(&fixture(), &params(5, 16), &surroundings);
        graph.check_invariants().unwrap();
        grown.push(inside_and_height(&graph, &sphere));
    }
    let [(analytic, analytic_top), (voxel, voxel_top)] = [grown[0], grown[1]];
    assert!(analytic > 60 && voxel > 60, "{analytic} {voxel}");
    assert!(
        voxel.abs_diff(analytic) * 4 < analytic,
        "{analytic} {voxel}"
    );
    assert!(analytic_top > centre.y + 0.5 * radius && voxel_top > centre.y + 0.5 * radius);
}
