//! Engine options of the self-organizing mode, measured on the presets that use it.
//!
//! Benchmarks are named `<group>/<preset>/<age>/<option>`:
//!
//! * `grid`: growth with the shadow grid stored `dense` or `sparse`;
//! * `buds`: growth with the buds sensed `sequential` or `parallel`;
//! * `markers`: growth with the markers given to buds `sequential` or `parallel`, for
//!   the presets that sense markers.
//!
//! Two more groups isolate the data structures: `cast/<storage>` casts and reads the
//! shadows of the nodes of one grown tree in an empty grid (the memory the grid ends
//! with is printed), and
//! `perceive/<sequential|parallel>` runs one cycle of marker association for the buds
//! of one grown tree. The node count of each plant, grown with seed 1, is printed
//! before it is measured. Every option gives the same plant; only the time differs.
//!
//! Run with `cargo bench -p arbrume-gen --bench engine`.

use arbrume_core::species::{GrowthMode, Sensing, Species};
use arbrume_gen::{preset, presets};
use arbrume_grow::{EngineOptions, GridStorage, GrowParams, Markers, Perception, ShadowGrid};
use arbrume_grow::{Surroundings, default_envelope, grow_in_with, shadow_params};
use criterion::{Criterion, criterion_group, criterion_main};
use glam::Vec3;
use std::hint::black_box;
use std::time::Duration;

const AGES: [u16; 3] = [12, 24, 36];

/// The presets in the self-organizing mode.
fn competing() -> Vec<(&'static str, Species)> {
    let all = presets().iter().map(|(name, _)| *name);
    all.filter_map(|name| preset(name).map(|species| (name, species)))
        .filter(|(_, species)| species.growth.mode == GrowthMode::SelfOrganizing)
        .collect()
}

fn params(age: u16) -> GrowParams {
    GrowParams {
        seed: 1,
        age,
        scale: 1.0,
    }
}

fn bench_options(criterion: &mut Criterion) {
    let open = Surroundings::none();
    let default = EngineOptions::default();
    let variants = [
        ("grid", "dense", default),
        (
            "grid",
            "sparse",
            EngineOptions {
                storage: GridStorage::Sparse,
                ..default
            },
        ),
        ("buds", "sequential", default),
        (
            "buds",
            "parallel",
            EngineOptions {
                parallel_buds: true,
                ..default
            },
        ),
        ("markers", "parallel", default),
        (
            "markers",
            "sequential",
            EngineOptions {
                parallel_markers: false,
                ..default
            },
        ),
    ];
    let mut group = criterion.benchmark_group("engine");
    group
        .sample_size(10)
        .measurement_time(Duration::from_secs(3));
    for (name, species) in competing() {
        let sensing = species.growth.self_organization.sensing;
        for age in AGES {
            let params = params(age);
            let nodes = grow_in_with(&species, &params, &open, default).node_count();
            println!("{name} age {age}: {nodes} nodes");
            for (what, option, options) in variants {
                let relevant = match what {
                    "markers" => sensing != Sensing::Shadow,
                    _ => sensing != Sensing::Markers,
                };
                if !relevant {
                    continue;
                }
                group.bench_function(format!("{what}/{name}/{age}/{option}"), |bencher| {
                    bencher.iter(|| grow_in_with(black_box(&species), &params, &open, options));
                });
            }
        }
    }
    group.finish();
}

fn bench_structures(criterion: &mut Criterion) {
    let open = Surroundings::none();
    let species = preset("open_grown_broadleaf").expect("built-in preset");
    let tree = grow_in_with(&species, &params(30), &open, EngineOptions::default());
    let points = tree.positions();
    println!("cast: {} nodes", points.len());
    let mut group = criterion.benchmark_group("structures");
    group
        .sample_size(10)
        .measurement_time(Duration::from_secs(3));
    for (label, storage) in [
        ("dense", GridStorage::Dense),
        ("sparse", GridStorage::Sparse),
    ] {
        let mut grid = ShadowGrid::new(shadow_params(&species, 1.0), storage);
        for &point in points {
            grid.cast(point, 1.0);
        }
        let megabytes = grid.allocated_cells() as f64 * 4.0 / 1e6;
        println!("cast/{label}: {megabytes:.2} MB of voxels");
        group.bench_function(format!("cast/{label}"), |bencher| {
            bencher.iter(|| {
                let mut grid = ShadowGrid::new(shadow_params(&species, 1.0), storage);
                for &point in points {
                    grid.cast(point, 1.0);
                }
                let light: f32 = points.iter().map(|&p| grid.exposure(p, 0.0)).sum();
                black_box(light)
            });
        });
    }
    let crown = preset("envelope_tree").expect("built-in preset");
    let unit = crown.growth.internode_unit();
    let model = &crown.growth.self_organization;
    let perception = Perception {
        occupancy: model.occupancy_radius * unit,
        reach: model.perception_distance * unit,
        angle: model.perception_angle_deg.to_radians(),
    };
    let density = model.marker_density / unit.powi(3);
    let envelope = default_envelope(&crown, 1.0);
    let markers = Markers::scatter(&envelope, 1, density, model.marker_limit);
    let buds = grow_in_with(&crown, &params(8), &open, EngineOptions::default());
    let directions = vec![Vec3::Y; buds.node_count()];
    println!(
        "perceive: {} markers, {} buds",
        markers.points().len(),
        buds.node_count()
    );
    for (label, parallel) in [("sequential", false), ("parallel", true)] {
        group.bench_function(format!("perceive/{label}"), |bencher| {
            bencher.iter(|| {
                let mut markers = markers.clone();
                let seen = markers.perceive(buds.positions(), &directions, &perception, parallel);
                black_box(seen.len())
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_options, bench_structures);
criterion_main!(benches);
