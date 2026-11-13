//! Properties that hold for every seed and age: structural invariants, determinism and
//! the relation between a plant and the same plant grown longer.

mod common;

use arbrume_core::curve::OrderCurve;
use arbrume_core::graph::PlantGraph;
use arbrume_core::species::Species;
use arbrume_grow::{GrowParams, grow};
use common::{STARTERS, starter};
use glam::Vec3;
use proptest::prelude::*;
use std::collections::HashSet;

/// The starter species and the default species.
fn species_pool() -> Vec<Species> {
    let mut pool: Vec<Species> = STARTERS.iter().map(|name| starter(name)).collect();
    pool.push(Species::default());
    pool
}

/// A species whose growth does not look at the age of the plant and is not bent
/// afterwards, so that a young plant is a prefix of an older one.
fn without_age_effects(mut species: Species) -> Species {
    species.growth.bare_base = OrderCurve::constant(0.0);
    species.growth.weight_bending = 0.0;
    species
}

/// Every stored number of a graph as bits, for bit-exact comparison.
fn fingerprint(graph: &PlantGraph) -> Vec<u32> {
    let positions = graph.positions().iter().flat_map(Vec3::to_array);
    let mut bits: Vec<u32> = positions.map(f32::to_bits).collect();
    bits.extend(graph.radii().iter().map(|r| r.to_bits()));
    bits.extend(graph.parents().iter().map(|parent| parent.0));
    bits.extend(graph.births().iter().map(|&birth| u32::from(birth)));
    for site in graph.organs() {
        bits.push(site.node.0);
        bits.extend(site.direction.to_array().map(f32::to_bits));
        bits.extend(site.normal.to_array().map(f32::to_bits));
        bits.push(site.scale.to_bits());
    }
    bits
}

/// Asserts that `young`, grown for `age` cycles, is the part of `old` born before cycle
/// `age`: same chains, same nodes at the same places.
fn assert_prefix(young: &PlantGraph, old: &PlantGraph, age: u16) {
    let born_before = old.births().iter().filter(|&&birth| birth < age).count();
    assert_eq!(young.node_count(), born_before);
    assert!(young.chain_count() <= old.chain_count());
    for chain in old.chain_ids() {
        let view = old.chain_view(chain).unwrap();
        let kept = age.checked_sub(1).map_or(0, |last| view.born_by(last));
        if chain.index() >= young.chain_count() {
            assert_eq!(kept, 0, "a chain missing from the young plant is older");
            continue;
        }
        let early = young.chain_view(chain).unwrap();
        assert_eq!(early.positions, &view.positions[..kept]);
        assert_eq!((early.order, early.origin), (view.order, view.origin));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn grown_plants_are_well_formed(
        seed in any::<u64>(),
        age in 0_u16..28,
        pick in 0_usize..4,
        scale in 0.25_f32..4.0,
    ) {
        let species = &species_pool()[pick];
        let graph = grow(species, &GrowParams { seed, age, scale });
        prop_assert_eq!(graph.check_invariants(), Ok(()));
        prop_assert!(graph.is_contiguous());
        prop_assert_eq!(graph.node_count() == 0, age == 0);
        prop_assert!(graph.radii().iter().all(|&r| r.is_finite() && r > 0.0));
        prop_assert!(graph.births().iter().all(|&birth| birth < age));
        for site in graph.organs() {
            prop_assert!((site.direction.length() - 1.0).abs() < 1e-3);
            prop_assert!(site.direction.dot(site.normal).abs() < 1e-2);
        }
    }

    #[test]
    fn the_same_inputs_give_the_same_bits(
        seed in any::<u64>(),
        age in 1_u16..24,
        pick in 0_usize..4,
    ) {
        let pool = species_pool();
        let params = GrowParams { seed, age, scale: 1.0 };
        let reference = grow(&pool[pick], &params);
        let other = grow(&pool[(pick + 1) % 4], &GrowParams { seed: seed ^ 1, ..params });
        prop_assert!(other.node_count() > 0);
        let again = grow(&pool[pick], &params);
        prop_assert_eq!(&reference, &again);
        prop_assert_eq!(fingerprint(&reference), fingerprint(&again));
    }

    #[test]
    fn a_young_plant_is_a_prefix_of_the_older_one(
        seed in any::<u64>(),
        young in 0_u16..14,
        extra in 1_u16..10,
        pick in 0_usize..4,
    ) {
        let species = without_age_effects(species_pool().swap_remove(pick));
        let params = GrowParams { seed, age: young, scale: 1.0 };
        let early = grow(&species, &params);
        let late = grow(&species, &GrowParams { age: young + extra, ..params });
        assert_prefix(&early, &late, young);
    }

    #[test]
    fn an_older_plant_only_loses_laterals_of_its_bare_bases(
        seed in any::<u64>(),
        young in 1_u16..14,
        extra in 1_u16..8,
        pick in 0_usize..4,
    ) {
        let mut species = species_pool().swap_remove(pick);
        species.growth.weight_bending = 0.0;
        let params = GrowParams { seed, age: young, scale: 1.0 };
        let early = grow(&species, &params);
        let late = grow(&species, &GrowParams { age: young + extra, ..params });
        let place = |graph: &PlantGraph, node: usize| {
            (graph.orders()[node], graph.positions()[node].to_array().map(f32::to_bits))
        };
        let known: HashSet<_> = (0..early.node_count()).map(|node| place(&early, node)).collect();
        let old = (0..late.node_count()).filter(|&node| late.births()[node] < young);
        for node in old {
            prop_assert!(known.contains(&place(&late, node)), "node {} is new", node);
        }
        let stems = |graph: &PlantGraph| {
            let births = graph.births().iter().zip(graph.orders());
            births.filter(|&(&birth, &order)| order == 0 && birth < young).count()
        };
        prop_assert_eq!(stems(&early), stems(&late));
    }
}

#[test]
fn growing_on_several_threads_gives_the_plants_of_one_thread() {
    let pool = species_pool();
    let jobs: Vec<(usize, u64)> = (0..12)
        .map(|job| (job % pool.len(), job as u64 * 77))
        .collect();
    let run = |&(pick, seed): &(usize, u64)| {
        let params = GrowParams {
            seed,
            age: 14,
            scale: 1.0,
        };
        fingerprint(&grow(&pool[pick], &params))
    };
    let sequential: Vec<Vec<u32>> = jobs.iter().map(run).collect();
    let threaded: Vec<Vec<u32>> = std::thread::scope(|scope| {
        let handles: Vec<_> = jobs
            .iter()
            .rev()
            .map(|job| scope.spawn(move || run(job)))
            .collect();
        let results = handles.into_iter().map(|handle| handle.join().unwrap());
        results.rev().collect()
    });
    assert_eq!(sequential, threaded);
}
