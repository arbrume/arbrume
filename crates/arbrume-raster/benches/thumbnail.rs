//! Thumbnail rendering time on one thread.
//!
//! Two plants of the starter broadleaf species: a young one (age 14, about 35k
//! triangles) and a grown one meshed finer (age 30, quality 1.5, about 300k triangles),
//! each rendered at 256 by 256 and 512 by 512 pixels with the default thumbnail options
//! (3x3 supersampling, shadows, sky occlusion, ground) by a reused [`Renderer`].
//!
//! Run with `cargo bench -p arbrume-raster`.

use arbrume_core::mesh::TreeMesh;
use arbrume_core::species::Species;
use arbrume_grow::{GrowParams, grow};
use arbrume_mesh::{MeshOptions, mesh_tree};
use arbrume_raster::{Renderer, ThumbnailOptions};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Duration;

fn broadleaf(age: u16, quality: f32) -> TreeMesh {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../arbrume-grow/tests/data/broadleaf.toml");
    let species = Species::from_path(path).expect("starter species");
    let graph = grow(
        &species,
        &GrowParams {
            seed: 1,
            age,
            scale: 1.0,
        },
    );
    let options = MeshOptions {
        seed: 1,
        ..MeshOptions::with_quality(quality)
    };
    mesh_tree(&graph, &species, &options).expect("starter species meshes")
}

fn bench_thumbnail(criterion: &mut Criterion) {
    let plants = [broadleaf(14, 1.0), broadleaf(30, 1.5)];
    let mut group = criterion.benchmark_group("thumbnail");
    group
        .sample_size(20)
        .measurement_time(Duration::from_secs(5));
    for mesh in &plants {
        for size in [256, 512] {
            let options = ThumbnailOptions {
                width: size,
                height: size,
                ..ThumbnailOptions::default()
            };
            let mut renderer = Renderer::new();
            let id = BenchmarkId::new(
                format!("{size}px"),
                format!("{}_triangles", mesh.triangle_count()),
            );
            group.bench_function(id, |bencher| {
                bencher.iter(|| renderer.thumbnail(black_box(mesh), &options));
            });
        }
    }
    group.finish();
}

criterion_group!(benches, bench_thumbnail);
criterion_main!(benches);
