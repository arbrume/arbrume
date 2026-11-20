//! Stands: order independence, thread independence, per-species shadow coefficients and
//! markers that follow their plant.

mod common;

use arbrume_core::graph::PlantGraph;
use arbrume_core::species::{Sensing, Species};
use arbrume_grow::{
    EngineOptions, GridStorage, GrowParams, ShadowGrid, ShadowParams, Standing, Surroundings, grow,
    grow_together, grow_together_with,
};
use common::starter;
// The fixture list of the other test files.
const _: [&str; 3] = common::STARTERS;
use glam::Vec3;

/// The competing fixture with the given sensing, renamed `name`.
fn competing(name: &str, sensing: Sensing) -> Species {
    let mut species = starter("competing");
    name.clone_into(&mut species.name);
    species.growth.self_organization.sensing = sensing;
    if sensing != Sensing::Shadow {
        species.growth.self_organization.shedding_threshold = 0.0;
    }
    species.validate().unwrap();
    species
}

/// The fixture with coarser voxels, a stronger and shallower shadow.
fn coarse() -> Species {
    let mut species = competing("coarse", Sensing::Shadow);
    let model = &mut species.growth.self_organization;
    model.shadow_cell *= 1.5;
    model.shadow_strength *= 1.4;
    model.shadow_depth = model.shadow_depth.saturating_sub(2).max(1);
    species.validate().unwrap();
    species
}

fn params(seed: u64, age: u16) -> GrowParams {
    GrowParams {
        seed,
        age,
        scale: 1.0,
    }
}

/// Node positions, radii and parents as bits.
fn fingerprint(graph: &PlantGraph) -> Vec<u32> {
    let positions = graph.positions().iter().flat_map(Vec3::to_array);
    let mut bits: Vec<u32> = positions.map(f32::to_bits).collect();
    bits.extend(graph.radii().iter().map(|r| r.to_bits()));
    bits.extend(graph.parents().iter().map(|parent| parent.0));
    bits.extend(graph.flags().iter().map(|flags| u32::from(flags.0)));
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

/// The four species of the mixed stand: two shadow species with different
/// coefficients, one sensing both, one parametric.
fn mixed_species() -> [Species; 4] {
    [
        competing("fine", Sensing::Shadow),
        coarse(),
        competing("both", Sensing::Both),
        starter("shrub"),
    ]
}

/// A mixed stand of five plants, close enough to shade one another.
fn mixed(species: &[Species; 4]) -> Vec<Standing<'_>> {
    let at = |kind: usize, seed: u64, age: u16, x: f32, z: f32| Standing {
        species: &species[kind],
        params: params(seed, age),
        position: Vec3::new(x, 0.0, z),
    };
    vec![
        at(0, 1, 11, -1.0, 0.0),
        at(1, 2, 12, 0.6, 0.3),
        at(2, 3, 10, 0.0, 1.2),
        at(3, 4, 8, 1.5, -0.5),
        at(0, 5, 9, -0.4, -1.1),
    ]
}

/// Every permutation of `0..n`.
fn permutations(n: usize) -> Vec<Vec<usize>> {
    if n == 0 {
        return vec![Vec::new()];
    }
    let mut all = Vec::new();
    for rest in permutations(n - 1) {
        for slot in 0..n {
            let mut order = rest.clone();
            order.insert(slot, n - 1);
            all.push(order);
        }
    }
    all
}

#[test]
fn the_order_of_the_entries_does_not_matter() {
    let species = mixed_species();
    let stand = mixed(&species);
    let open = Surroundings::none();
    let want: Vec<Vec<u32>> = grow_together(&stand, &open)
        .iter()
        .map(fingerprint)
        .collect();
    let orders = permutations(stand.len());
    assert_eq!(orders.len(), 120);
    for order in orders.iter().step_by(7) {
        let permuted: Vec<Standing> = order.iter().map(|&index| stand[index]).collect();
        let graphs = grow_together(&permuted, &open);
        for (graph, &index) in graphs.iter().zip(order) {
            assert_eq!(fingerprint(graph), want[index], "order {order:?}");
        }
    }
}

#[test]
fn a_stand_does_not_depend_on_threads_or_storage() {
    let species = mixed_species();
    let stand = mixed(&species);
    let open = Surroundings::none();
    let want: Vec<Vec<u32>> = grow_together(&stand, &open)
        .iter()
        .map(fingerprint)
        .collect();
    for threads in [1, 3] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let sparse = EngineOptions {
            storage: GridStorage::Sparse,
            parallel_buds: true,
            parallel_markers: false,
        };
        for options in [EngineOptions::default(), sparse] {
            let graphs = pool.install(|| grow_together_with(&stand, &open, options));
            let got: Vec<Vec<u32>> = graphs.iter().map(fingerprint).collect();
            assert_eq!(got, want, "{threads} threads, {options:?}");
        }
    }
}

#[test]
fn neighbours_shade_each_other_and_parametric_plants_are_unchanged() {
    let species = mixed_species();
    let stand = mixed(&species);
    let graphs = grow_together(&stand, &Surroundings::none());
    for (graph, standing) in graphs.iter().zip(&stand) {
        graph.check_invariants().unwrap();
        let alone = grow(standing.species, &standing.params);
        let same = fingerprint(graph) == fingerprint(&alone);
        let parametric = standing.species.name == species[3].name;
        assert_eq!(same, parametric, "{}", standing.species.name);
    }
}

#[test]
fn a_coarser_species_casts_its_own_pyramid_into_a_finer_grid() {
    let fine = ShadowParams {
        cell: 0.5,
        strength: 0.2,
        decay: 2.0,
        depth: 4,
        full: 1.0,
    };
    assert_eq!(fine.in_cell(0.5), fine);
    let coarse = ShadowParams {
        cell: 1.0,
        strength: 0.3,
        ..fine
    };
    let rescaled = coarse.in_cell(0.5);
    assert_eq!(rescaled.depth, 8);
    assert!((rescaled.decay - 2.0_f32.sqrt()).abs() < 1e-6);
    let mut grid = ShadowGrid::new(fine, GridStorage::Dense);
    grid.set_coefficients(&coarse);
    let top = Vec3::new(0.25, 4.25, 0.25);
    grid.cast(top, 1.0);
    let cell = grid.cell_of(top);
    assert!((grid.shadow(cell) - 0.3).abs() < 1e-6);
    let below = cell - glam::IVec3::Y * 8;
    assert!((grid.shadow(below) - 0.3 / 16.0).abs() < 1e-6);
    assert_eq!(grid.shadow(below - glam::IVec3::Y), 0.0);
    grid.set_coefficients(&fine);
    assert_eq!(grid.params(), &fine);
    grid.set_coefficients(&coarse);
    grid.cast(top, -1.0);
    assert!(grid.shadow(cell).abs() < 1e-6 && grid.shadow(below).abs() < 1e-6);
}

#[test]
fn markers_follow_a_plant_standing_away_from_the_origin() {
    let species = competing("markers", Sensing::Markers);
    let position = Vec3::new(6.0, 0.0, -3.0);
    let stand = [Standing {
        species: &species,
        params: params(2, 12),
        position,
    }];
    let moved = grow_together(&stand, &Surroundings::none());
    let alone = grow(&species, &params(2, 12));
    assert!(alone.node_count() > 100);
    let ratio = moved[0].node_count() as f32 / alone.node_count() as f32;
    assert!((0.9..1.1).contains(&ratio), "{ratio}");
    let shift = (centroid(&moved[0]) - centroid(&alone)).length();
    assert!(shift < 0.1, "{shift}");
}
