//! The starter species files load, validate and grow into well-formed plants.

mod common;

use arbrume_core::graph::{NodeFlags, OrganKind, PlantGraph};
use arbrume_core::species::{RadiusModel, Species};
use arbrume_grow::{GrowParams, grow};
use common::{STARTERS, starter};

fn grown(species: &Species, age: u16) -> PlantGraph {
    let params = GrowParams {
        seed: 3,
        age,
        scale: 1.0,
    };
    let graph = grow(species, &params);
    graph.check_invariants().unwrap();
    assert!(graph.is_contiguous());
    graph
}

#[test]
fn starter_species_round_trip_through_toml() {
    for name in STARTERS {
        let species = starter(name);
        let text = species.to_toml_string().unwrap();
        assert_eq!(Species::from_toml_str(&text).unwrap(), species, "{name}");
    }
}

#[test]
fn starter_species_grow_with_radii_and_leaf_sites() {
    for name in STARTERS {
        let species = starter(name);
        for age in [1, 5, 15, 30] {
            let graph = grown(&species, age);
            assert!(graph.node_count() > 0, "{name} at {age}");
            let radii = graph.radii();
            assert!(radii.iter().all(|&r| r.is_finite() && r > 0.0), "{name}");
            assert!(!graph.organs().is_empty(), "{name} at {age} has no leaf");
            for site in graph.organs() {
                assert_eq!(site.kind, OrganKind::Leaf);
                assert!((site.direction.length() - 1.0).abs() < 1e-4);
                assert!((site.normal.length() - 1.0).abs() < 1e-4);
                assert!(site.direction.dot(site.normal).abs() < 1e-3);
                assert!(site.scale > 0.0 && !graph.is_shed(site.node));
            }
        }
    }
}

#[test]
fn radii_never_increase_towards_the_tips() {
    for name in STARTERS {
        let species = starter(name);
        let graph = grown(&species, 25);
        let radii = graph.radii();
        for node in graph.node_ids() {
            if let Some(parent) = graph.parent(node) {
                let (child, bearer) = (radii[node.index()], radii[parent.index()]);
                assert!(child <= bearer * (1.0 + 1e-4), "{name}: {child} > {bearer}");
            }
        }
    }
}

#[test]
fn the_tapered_trunk_base_follows_the_radius_ratio() {
    let species = starter("conifer");
    assert_eq!(species.geometry.radius_model, RadiusModel::Tapered);
    let graph = grown(&species, 40);
    let trunk = graph.chain_view(graph.chain_of(arbrume_core::graph::NodeId::new(0)));
    let trunk = trunk.unwrap();
    let length = trunk.length();
    let first = trunk.positions[0].distance(trunk.origin);
    let taper = species.geometry.taper.eval(0, first / length);
    let expected = species.geometry.radius_ratio * length * taper;
    assert!((trunk.radii[0] - expected).abs() < expected * 1e-3);
    let tip = species.geometry.radius_ratio * length * species.geometry.taper.eval(0, 1.0);
    assert!((trunk.radii[trunk.len() - 1] - tip).abs() < tip * 1e-3);
}

#[test]
fn rhythmic_species_flag_one_growth_unit_per_cycle_on_the_trunk() {
    let species = starter("conifer");
    let age = 12;
    let graph = grown(&species, age);
    let trunk = graph.chain_range(graph.chain_of(arbrume_core::graph::NodeId::new(0)));
    let flags = &graph.flags()[trunk.unwrap()];
    let units = flags
        .iter()
        .filter(|f| f.contains(NodeFlags::GROWTH_UNIT_START));
    assert_eq!(units.count(), usize::from(age));
}

#[test]
fn the_shrub_has_several_stems_and_relay_axes() {
    let species = starter("shrub");
    let graph = grown(&species, 10);
    let chains = graph.chains();
    let stems = chains.iter().filter(|chain| chain.attach.is_none()).count();
    assert_eq!(stems, usize::from(species.growth.stems));
    let relays = chains.iter().filter(|chain| {
        chain.attach.is_some() && graph.orders()[chain.attach.index()] == chain.order
    });
    assert!(relays.count() > stems);
    let origins = chains.iter().filter(|chain| chain.attach.is_none());
    for chain in origins {
        let spread = chain.origin.length();
        assert!((spread - species.growth.stem_spread).abs() < 1e-5);
        assert!(chain.origin.y.abs() < 1e-6);
    }
}

#[test]
fn scale_multiplies_positions_and_radii_exactly() {
    for name in STARTERS {
        let mut species = starter(name);
        species.growth.weight_bending = 0.0;
        let params = GrowParams {
            seed: 11,
            age: 9,
            scale: 1.0,
        };
        let unit = grow(&species, &params);
        let doubled = grow(
            &species,
            &GrowParams {
                scale: 2.0,
                ..params
            },
        );
        assert_eq!(unit.node_count(), doubled.node_count());
        let positions = unit.positions().iter().zip(doubled.positions());
        for (a, b) in positions {
            assert_eq!((*a * 2.0).to_array(), b.to_array(), "{name}");
        }
        let radii = unit.radii().iter().zip(doubled.radii());
        for (a, b) in radii {
            assert!((a * 2.0 - b).abs() <= b * 1e-5, "{name}");
        }
        assert_eq!(unit.organs().len(), doubled.organs().len());
    }
}
