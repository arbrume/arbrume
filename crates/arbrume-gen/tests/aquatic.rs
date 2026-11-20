//! Aquatic presets: under the surface, on it, bent by the current, spread by rhizomes.

use arbrume_core::graph::{ChainId, NodeId, PlantGraph};
use arbrume_core::species::Species;
use arbrume_gen::{GenParams, Plant, Surroundings, generate, preset};
use arbrume_grow::Water;
use arbrume_grow::aquatic::water_of;
use glam::Vec3;

const AQUATIC: [&str; 6] = [
    "kelp_like",
    "bull_kelp_like",
    "seagrass_like",
    "ribbon_grass_like",
    "waterweed_like",
    "water_lily_like",
];

/// Tolerance on heights at the surface, in meters: pads sit a few millimeters above it.
const ON_SURFACE: f32 = 0.01;

fn in_water(water: Water) -> Surroundings {
    Surroundings {
        water: Some(water),
        ..Surroundings::none()
    }
}

fn grown(name: &str, age: u16, surroundings: Option<Surroundings>) -> (Species, Plant) {
    let species = preset(name).unwrap();
    let mut params = GenParams::new(4, age).with_quality(0.5);
    if let Some(surroundings) = surroundings {
        params = params.with_surroundings(surroundings);
    }
    let plant = generate(&species, &params).unwrap();
    (species, plant)
}

fn pool(threads: usize) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
}

#[test]
fn aquatic_presets_stay_under_the_surface_and_float_on_it() {
    for name in AQUATIC {
        let (species, plant) = grown(name, species_age(name), None);
        let graph = &plant.graph;
        graph.check_invariants().unwrap();
        assert!(graph.is_contiguous(), "{name}");
        let surface = water_of(species.aquatic.as_ref().unwrap(), &Surroundings::none()).level();
        let highest = graph
            .positions()
            .iter()
            .map(|p| p.y)
            .fold(f32::MIN, f32::max);
        assert!(
            highest <= surface + ON_SURFACE,
            "{name}: a node at {highest}"
        );
        let near = |y: f32| (y - surface).abs() < 0.12;
        let at_surface = graph.positions().iter().filter(|p| near(p.y)).count();
        if matches!(
            name,
            "kelp_like" | "bull_kelp_like" | "water_lily_like" | "waterweed_like"
        ) {
            assert!(at_surface > 0, "{name} has nothing at the surface");
        }
        if name == "water_lily_like" {
            for site in graph.organs() {
                let y = graph.position(site.node).y;
                assert!((y - surface).abs() < ON_SURFACE, "{name}: a pad at {y}");
                assert!(site.normal.y > 0.999 && site.direction.y.abs() < 1e-3);
            }
        }
    }
}

/// The middle of the range of ages of preset `name`.
fn species_age(name: &str) -> u16 {
    let [young, old] = preset(name).unwrap().variation.age;
    young.midpoint(old)
}

#[test]
fn aquatic_plants_do_not_depend_on_the_number_of_threads() {
    for name in AQUATIC {
        let species = preset(name).unwrap();
        let params = GenParams::new(9, species_age(name)).with_quality(0.4);
        let reference = pool(1).install(|| generate(&species, &params).unwrap());
        for threads in [3, 8] {
            let plant = pool(threads).install(|| generate(&species, &params).unwrap());
            assert_eq!(plant, reference, "{name} on {threads} threads");
        }
    }
}

/// Mean position of the nodes of `graph`.
fn mean(graph: &PlantGraph) -> Vec3 {
    graph.positions().iter().copied().sum::<Vec3>() / graph.node_count() as f32
}

#[test]
fn a_current_bends_the_stems_downstream() {
    for name in ["kelp_like", "bull_kelp_like", "waterweed_like"] {
        let depth = preset(name).unwrap().aquatic.unwrap().water_depth;
        let still = grown(name, 12, Some(in_water(Water::still(depth)))).1;
        let current = Water {
            current: Vec3::new(0.0, 0.0, 0.25),
            ..Water::still(depth)
        };
        let flowing = grown(name, 12, Some(in_water(current))).1;
        let shift = mean(&flowing.graph) - mean(&still.graph);
        assert!(shift.z > 0.05 * depth, "{name}: shift {shift}");
        assert!(shift.z > 4.0 * shift.x.abs(), "{name}: shift {shift}");
        let highest = flowing
            .graph
            .positions()
            .iter()
            .map(|p| p.y)
            .fold(f32::MIN, f32::max);
        assert!(highest <= depth + ON_SURFACE, "{name}");
    }
}

#[test]
fn ramets_are_spaced_along_their_rhizome_and_joined_to_it() {
    let (species, plant) = grown("seagrass_like", 10, None);
    let rhizome = species.aquatic.unwrap().rhizome.unwrap();
    let graph = &plant.graph;
    let buried = |node: NodeId| graph.position(node).y < 0.0;
    let mut bearers = Vec::new();
    for (index, chain) in graph.chains().iter().enumerate() {
        if buried(chain.first) {
            let nodes: Vec<NodeId> = graph.chain_nodes_rev(ChainId::new(index)).collect();
            for &node in nodes.iter().rev().skip(1) {
                let step = graph.internode_length(node);
                assert!((step - rhizome.spacing).abs() < 1e-4, "rhizome step {step}");
            }
        } else if !chain.attach.is_none() && buried(chain.attach) {
            assert_eq!(graph.orders()[chain.attach.index()], chain.order);
            bearers.push(chain.attach);
        }
    }
    bearers.sort_unstable_by_key(|node| node.index());
    bearers.dedup();
    assert!(bearers.len() + 1 >= 20, "{} ramets", bearers.len() + 1);
    let stems = graph.chains().iter().filter(|c| c.attach.is_none()).count();
    assert_eq!(
        stems,
        rhizome.axes as usize + 1,
        "one ramet and the rhizomes at the base"
    );
}

#[test]
fn light_fades_with_depth_and_land_plants_ignore_water() {
    let depth = 1.4;
    let clear = Water {
        attenuation: 0.0,
        ..Water::still(depth)
    };
    let murky = Water {
        attenuation: 2.5,
        ..Water::still(depth)
    };
    let (_, bright) = grown("waterweed_like", 14, Some(in_water(clear)));
    let (_, dark) = grown("waterweed_like", 14, Some(in_water(murky)));
    let (lit, shaded) = (bright.graph.node_count(), dark.graph.node_count());
    assert!(
        2 * shaded < lit,
        "{shaded} nodes in murky water, {lit} in clear water"
    );
    assert!(bright.stats.height > 0.9 * depth && dark.stats.height < 0.5 * depth);
    let (_, dry) = grown("open_grown_broadleaf", 8, None);
    let (_, wet) = grown("open_grown_broadleaf", 8, Some(in_water(murky)));
    assert_eq!((dry.graph, dry.mesh), (wet.graph, wet.mesh));
}
