//! Meshing throughput on synthetic skeletons of three sizes, on one thread and on the
//! global `rayon` pool.
//!
//! Throughput is reported in triangles per second. Per size:
//!
//! * `plan`: the counting pass alone (`Mesher::counts` on a warm mesher);
//! * `fresh_sequential`, `fresh_parallel`: `mesh_tree`, which allocates its working
//!   memory and its output on every call;
//! * `reused_sequential`, `reused_parallel`: `Mesher::mesh_into` on a warm mesher and a
//!   warm output, which allocates nothing.
//!
//! The group `junctions` meshes the same skeletons with Tier A and with Tier B junctions
//! (`tier_a`, `tier_b`, reused buffers, one thread), the bark alone (no leaves), with the
//! throughput in Tier A triangles per second so that the times compare directly.
//!
//! Run with `cargo bench -p arbrume-mesh`.

use arbrume_core::mesh::TreeMesh;
use arbrume_core::species::Species;
use arbrume_mesh::synthetic::{SkeletonParams, skeleton};
use arbrume_mesh::{Junctions, MeshOptions, Mesher, mesh_tree};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::time::Duration;

fn bench_mesh(criterion: &mut Criterion) {
    let species = Species::default();
    let sizes = [
        SkeletonParams::small(1),
        SkeletonParams::medium(1),
        SkeletonParams::large(1),
    ];
    let mut group = criterion.benchmark_group("mesh_tree");
    group
        .sample_size(20)
        .measurement_time(Duration::from_secs(4));
    for params in sizes {
        let graph = skeleton(&params);
        let mut mesher = Mesher::new();
        let mut mesh = TreeMesh::default();
        let sequential = MeshOptions::default();
        let counts = mesher
            .counts(&graph, &species, &sequential)
            .expect("valid synthetic skeleton");
        let label = format!(
            "{}_nodes_{}_leaves_{}_triangles",
            graph.node_count(),
            counts.leaves,
            counts.triangles()
        );
        group.throughput(Throughput::Elements(counts.triangles() as u64));
        group.bench_function(BenchmarkId::new("plan", &label), |bencher| {
            bencher.iter(|| mesher.counts(black_box(&graph), &species, &sequential));
        });
        for (name, parallel) in [("sequential", false), ("parallel", true)] {
            let options = MeshOptions {
                parallel,
                ..sequential
            };
            let fresh = BenchmarkId::new(format!("fresh_{name}"), &label);
            group.bench_function(fresh, |bencher| {
                bencher.iter(|| mesh_tree(black_box(&graph), &species, &options));
            });
            let reused = BenchmarkId::new(format!("reused_{name}"), &label);
            group.bench_function(reused, |bencher| {
                bencher.iter(|| mesher.mesh_into(black_box(&graph), &species, &options, &mut mesh));
            });
        }
    }
    group.finish();
}

fn bench_junctions(criterion: &mut Criterion) {
    let species = Species::default();
    let mut group = criterion.benchmark_group("junctions");
    group
        .sample_size(20)
        .measurement_time(Duration::from_secs(4));
    for params in [SkeletonParams::medium(1), SkeletonParams::large(1)] {
        let graph = skeleton(&params);
        let mut mesher = Mesher::new();
        let mut mesh = TreeMesh::default();
        let tier_a = MeshOptions {
            leaf_fraction: 0.0,
            ..MeshOptions::default()
        };
        let counts = mesher
            .counts(&graph, &species, &tier_a)
            .expect("valid synthetic skeleton");
        let label = format!(
            "{}_nodes_{}_bark_triangles",
            graph.node_count(),
            counts.bark_triangles
        );
        group.throughput(Throughput::Elements(counts.bark_triangles as u64));
        for (name, junctions) in [("tier_a", Junctions::TierA), ("tier_b", Junctions::TierB)] {
            let options = MeshOptions {
                junctions,
                ..tier_a
            };
            group.bench_function(BenchmarkId::new(name, &label), |bencher| {
                bencher.iter(|| mesher.mesh_into(black_box(&graph), &species, &options, &mut mesh));
            });
        }
    }
    group.finish();
}

criterion_group!(benches, bench_mesh, bench_junctions);
criterion_main!(benches);
