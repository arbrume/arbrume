//! Climbing presets: structure, determinism over thread counts, stems held at the gap
//! from their support, hanging and creeping without one, and export.

use arbrume_core::graph::{ChainId, PlantGraph};
use arbrume_core::species::Species;
use arbrume_export::{GlbOptions, write_glb};
use arbrume_gen::{GenParams, Occluder, Plant, Shape, Surroundings, generate, preset};
use arbrume_grow::{GrowParams, grow_in};
use glam::Vec3;
use std::sync::Arc;

const CLIMBERS: [&str; 3] = ["ivy_like", "vine_like", "creeper_like"];

/// A wall 6 m wide and 4 m tall, its face 0.12 m behind the plant.
fn wall() -> Shape {
    Shape::Box {
        min: Vec3::new(-3.0, 0.0, 0.12),
        max: Vec3::new(3.0, 4.0, 0.42),
    }
}

/// A post of radius 6 cm, 0.1 m from the plant.
fn post() -> Shape {
    Shape::Cylinder {
        base: Vec3::new(0.0, 0.0, 0.16),
        radius: 0.06,
        height: 2.5,
    }
}

fn around(shape: Shape) -> Surroundings {
    Surroundings {
        envelope: None,
        occluders: vec![Occluder::obstacle(shape)],
        water: None,
    }
}

fn grown(name: &str, age: u16, surroundings: &Surroundings) -> PlantGraph {
    let species = preset(name).unwrap();
    let params = GrowParams {
        seed: 11,
        age,
        scale: 1.0,
    };
    grow_in(&species, &params, surroundings)
}

/// The live nodes of `graph`, with their radius.
fn live(graph: &PlantGraph) -> impl Iterator<Item = (Vec3, f32)> + '_ {
    (0..graph.node_count())
        .filter(|&index| !graph.flags()[index].is_shed())
        .map(|index| (graph.positions()[index], graph.radii()[index]))
}

#[test]
fn climbers_are_valid_in_every_setting() {
    for name in CLIMBERS {
        for surroundings in [Surroundings::none(), around(wall()), around(post())] {
            let graph = grown(name, 8, &surroundings);
            graph.check_invariants().unwrap();
            assert!(graph.is_contiguous(), "{name}");
            assert!(graph.node_count() > 50, "{name}: {}", graph.node_count());
            assert!(
                live(&graph).all(|(p, _)| p.is_finite() && p.y >= -1e-4),
                "{name}"
            );
        }
    }
}

#[test]
fn climbers_are_deterministic_over_thread_counts() {
    let species = preset("ivy_like").unwrap();
    let params = GenParams::new(5, 9).with_surroundings(around(wall()));
    let run = |threads: usize| -> Plant {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| generate(&species, &params).unwrap())
    };
    let one = run(1);
    for threads in [2, 8] {
        assert_eq!(run(threads), one, "{threads} threads");
    }
}

#[test]
fn ivy_climbs_a_wall_and_stays_at_the_gap() {
    let species = preset("ivy_like").unwrap();
    let climbing = species.climbing.clone().unwrap();
    let wall = wall();
    let graph = grown("ivy_like", 12, &around(wall.clone()));
    let (mut held, mut total, mut top) = (0, 0, 0.0_f32);
    let organ = 0.5 * species.geometry.tip_radius;
    for (point, radius) in live(&graph).filter(|&(_, radius)| radius > organ) {
        let distance = wall.distance(point);
        assert!(distance > radius, "a stem enters the wall at {point}");
        total += 1;
        if distance <= climbing.reach + radius + climbing.gap {
            held += 1;
            assert!(
                distance >= radius + climbing.gap - 1e-4,
                "{point}: {distance}"
            );
        }
        top = top.max(point.y);
    }
    assert!(held * 10 > total * 6, "{held} of {total} nodes held");
    assert!(top > 1.5, "the ivy reaches {top} m");
}

#[test]
fn vine_winds_up_a_post_with_tendrils() {
    let graph = grown("vine_like", 10, &around(post()));
    let post = post();
    let top = live(&graph).map(|(p, _)| p.y).fold(0.0, f32::max);
    assert!(top > 1.5, "the vine reaches {top} m");
    assert!(live(&graph).all(|(p, r)| post.distance(p) > r * 0.99));
}

#[test]
fn without_a_support_shoots_hang_or_creep() {
    let mut species = preset("ivy_like").unwrap();
    let params = GrowParams {
        seed: 3,
        age: 10,
        scale: 1.0,
    };
    let creeping = grow_in(&species, &params, &Surroundings::none());
    let height = |graph: &PlantGraph| live(graph).map(|(p, _)| p.y).fold(0.0, f32::max);
    assert!(height(&creeping) < 0.3, "{}", height(&creeping));
    let mut climbing = species.climbing.clone().unwrap();
    climbing.ground = false;
    species.climbing = Some(climbing);
    let hanging = grow_in(&species, &params, &Surroundings::none());
    let (mut long, mut hanging_tips) = (0, 0);
    for chain in 0..hanging.chain_count() {
        let Some(view) = hanging.chain_view(ChainId(chain as u32)) else {
            continue;
        };
        if view.positions.len() < 10 {
            continue;
        }
        long += 1;
        let top = view.positions.iter().map(|p| p.y).fold(f32::MIN, f32::max);
        let tip = view.positions[view.positions.len() - 1];
        hanging_tips += usize::from(tip.y < top - 0.1);
    }
    assert!(
        long > 3 && hanging_tips * 2 > long,
        "{hanging_tips} of {long}"
    );
}

#[test]
fn creeper_covers_rocks_without_entering_them() {
    let rocks = [
        Shape::Sphere {
            center: Vec3::new(0.6, -0.1, 0.2),
            radius: 0.35,
        },
        Shape::Ellipsoid {
            center: Vec3::new(-0.5, 0.0, -0.4),
            radii: Vec3::new(0.4, 0.25, 0.3),
        },
    ];
    let surroundings = Surroundings {
        envelope: None,
        occluders: rocks.iter().cloned().map(Occluder::obstacle).collect(),
        water: None,
    };
    let graph = grown("creeper_like", 10, &surroundings);
    let mut on_rock = 0;
    for (point, radius) in live(&graph) {
        let distance = rocks
            .iter()
            .map(|rock| rock.distance(point))
            .fold(f32::MAX, f32::min);
        assert!(distance > 0.0, "{point}");
        on_rock += usize::from(distance < radius + 0.06 && point.y > 0.1);
    }
    assert!(on_rock > 10, "{on_rock} nodes on the rocks");
}

#[test]
fn climber_files_round_trip_and_export_cleanly() {
    for name in CLIMBERS {
        let species = preset(name).unwrap();
        let text = species.to_toml_string().unwrap();
        assert_eq!(Species::from_toml_str(&text).unwrap(), species, "{name}");
        assert_eq!(
            species.variation.weight, 0.0,
            "{name} is not drawn at random"
        );
        let params = GenParams::new(2, 7).with_surroundings(Arc::new(around(wall())));
        let plant = generate(&species, &params).unwrap();
        let mut bytes = Vec::new();
        write_glb(&plant.mesh, &GlbOptions::default(), &mut bytes).unwrap();
        assert!(bytes.len() > 1000, "{name}");
    }
}
