//! Time of each stage of the pipeline, and of the whole, for every preset at a few ages.
//!
//! Benchmarks are named `<stage>/<preset>/<age>` with the stages `grow` (growth alone),
//! `mesh` (meshing the grown graph with a warm `Mesher` into a warm mesh), `export`
//! (writing the `.glb` with its metadata to a reused memory buffer) and `all` (the
//! three in turn with a warm `Generator`). The node and triangle counts and the file
//! size of each plant, grown with seed 1 at full quality, are printed before it is
//! measured. The ages are those of the `arbrume-grow` benchmark.
//!
//! Run with `cargo bench -p arbrume-gen`.

use arbrume_core::mesh::TreeMesh;
use arbrume_export::{GlbOptions, write_glb};
use arbrume_gen::{GENERATOR, GenParams, Generator, Plant, metadata, preset, presets};
use arbrume_grow::grow;
use arbrume_mesh::Mesher;
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::time::Duration;

const AGES: [u16; 3] = [8, 20, 40];

fn export(plant: &Plant, species: &arbrume_core::species::Species, out: &mut Vec<u8>) {
    out.clear();
    let options = GlbOptions {
        name: "plant".to_owned(),
        generator: GENERATOR.to_owned(),
        extras: Some(metadata(species, plant)),
    };
    write_glb(&plant.mesh, &options, out).expect("valid mesh");
}

fn bench_pipeline(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("pipeline");
    group
        .sample_size(10)
        .measurement_time(Duration::from_secs(3));
    for (name, _) in presets() {
        let species = preset(name).expect("built-in preset");
        for age in AGES {
            let params = GenParams::new(1, age);
            let mut generator = Generator::new();
            let mut plant = Plant::default();
            generator
                .generate_into(&species, &params, &mut plant)
                .expect("preset generates");
            let mut bytes = Vec::new();
            export(&plant, &species, &mut bytes);
            println!(
                "{name} age {age}: {} nodes, {} triangles, {:.2} MB",
                plant.stats.nodes,
                plant.stats.triangles,
                bytes.len() as f64 / 1e6
            );
            let label = format!("{name}/{age}");
            group.bench_function(format!("grow/{label}"), |bencher| {
                bencher.iter(|| grow(black_box(&species), &params.grow_params()));
            });
            let mut mesher = Mesher::new();
            let mut mesh = TreeMesh::default();
            group.bench_function(format!("mesh/{label}"), |bencher| {
                bencher.iter(|| {
                    mesher.mesh_into(black_box(&plant.graph), &species, &params.mesh, &mut mesh)
                });
            });
            group.bench_function(format!("export/{label}"), |bencher| {
                bencher.iter(|| export(black_box(&plant), &species, &mut bytes));
            });
            let mut scratch = Plant::default();
            group.bench_function(format!("all/{label}"), |bencher| {
                bencher.iter(|| {
                    generator
                        .generate_into(black_box(&species), &params, &mut scratch)
                        .expect("preset generates");
                    export(&scratch, &species, &mut bytes);
                });
            });
        }
    }
    group.finish();
}

criterion_group!(benches, bench_pipeline);
criterion_main!(benches);
