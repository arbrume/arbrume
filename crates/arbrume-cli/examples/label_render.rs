//! Renders plants coloured by their ML labels, to check the labels by eye.
//!
//! Usage: `cargo run --release -p arbrume --example label_render -- <out_dir> [preset ...]`
//!
//! For each preset (by default `broadleaf`, `conifer`, `excurrent_conifer` and
//! `palm_like`), grows plant 0 of base seed 1 at the middle of its age range and writes:
//!
//! * `<preset>_semantic.png`: trunk dark brown, branches orange, twig stems magenta,
//!   leaves green;
//! * `<preset>_instance.png`: one colour per instance (axis or organ site), drawn from
//!   its class and id;
//! * `<preset>_points.png`: a point cloud of 20 000 samples, each drawn as a small
//!   tetrahedron in the colour of its class;
//! * `<preset>_closeup.png`: the semantic colours of the triangles within 25 cm of the
//!   site of the median leaf instance, to see twig stems against their leaves.
//!
//! The vertex colours replace the material colours, so the images show labels only.

use std::path::PathBuf;

use arbrume_core::mesh::{MeshBuffers, MeshPart, PartKind, PartLabels, TreeMesh};
use arbrume_core::rng::mix64;
use arbrume_export::annotations::{MeshLabels, PointCloud, sample_points};
use arbrume_gen::{GenParams, Generator, plant_seed, preset};
use arbrume_raster::{Renderer, ThumbnailOptions};

const DEFAULT_PRESETS: [&str; 4] = ["broadleaf", "conifer", "excurrent_conifer", "palm_like"];

/// Linear RGB of each semantic class.
const CLASS_COLORS: [[f32; 3]; 4] = [
    [0.18, 0.07, 0.02],
    [0.95, 0.42, 0.05],
    [0.85, 0.05, 0.65],
    [0.12, 0.55, 0.08],
];

fn instance_color(class: u8, instance: u32) -> [f32; 3] {
    let hash = mix64((u64::from(class) << 32) | u64::from(instance));
    let channel = |shift: u32| 0.1 + 0.9 * ((hash >> shift) & 0xFF) as f32 / 255.0;
    [channel(0), channel(8), channel(16)]
}

/// `mesh` with every vertex coloured by `color(class, instance)` on a white material.
fn recolored(
    mesh: &TreeMesh,
    labels: &[PartLabels],
    color: impl Fn(u8, u32) -> [f32; 3],
) -> TreeMesh {
    let mut out = mesh.clone();
    for (part, own) in out.parts.iter_mut().zip(labels) {
        part.material.base_color = [1.0; 4];
        part.material.base_color_texture = None;
        part.material.alpha_cutoff = None;
        part.buffers.colors = own
            .semantic
            .iter()
            .zip(&own.instance)
            .map(|(&class, &instance)| {
                let [r, g, b] = color(class, instance);
                [r, g, b, 1.0]
            })
            .collect();
    }
    out
}

/// The triangles of `mesh` whose first corner lies within `radius` of `center`, with
/// only the vertices they use, so that a thumbnail frames them.
fn closeup(mesh: &TreeMesh, center: [f32; 3], radius: f32) -> TreeMesh {
    let mut out = mesh.clone();
    for part in &mut out.parts {
        let old = std::mem::take(&mut part.buffers);
        let near = |v: u32| {
            let p = old.positions[v as usize];
            (0..3).map(|k| (p[k] - center[k]).powi(2)).sum::<f32>() <= radius * radius
        };
        let mut remap = vec![u32::MAX; old.positions.len()];
        for triangle in old.indices.as_chunks::<3>().0.iter().filter(|t| near(t[0])) {
            for &v in triangle {
                if remap[v as usize] == u32::MAX {
                    remap[v as usize] = part.buffers.positions.len() as u32;
                    part.buffers.positions.push(old.positions[v as usize]);
                    part.buffers.normals.push(old.normals[v as usize]);
                    part.buffers.colors.push(old.colors[v as usize]);
                }
                part.buffers.indices.push(remap[v as usize]);
            }
        }
    }
    out.parts.retain(|part| !part.buffers.indices.is_empty());
    out
}

/// One small tetrahedron per point, coloured by class.
fn point_mesh(cloud: &PointCloud, size: f32) -> TreeMesh {
    let corners = [
        [1.0, 1.0, 1.0],
        [1.0, -1.0, -1.0],
        [-1.0, 1.0, -1.0],
        [-1.0, -1.0, 1.0],
    ];
    let mut buffers = MeshBuffers::default();
    for (point, &class) in cloud.position.iter().zip(&cloud.semantic) {
        let base = buffers.positions.len() as u32;
        let [r, g, b] = CLASS_COLORS[usize::from(class.min(3))];
        for corner in corners {
            let offset: [f32; 3] = std::array::from_fn(|k| point[k] + corner[k] * size);
            buffers.positions.push(offset);
            let length = 3.0_f32.sqrt();
            buffers.normals.push(corner.map(|c: f32| c / length));
            buffers.colors.push([r, g, b, 1.0]);
        }
        for face in [[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]] {
            buffers.indices.extend(face.map(|k| base + k));
        }
    }
    let mut part = MeshPart {
        kind: PartKind::Bark,
        material: arbrume_core::mesh::Material {
            name: "points".to_owned(),
            base_color: [1.0; 4],
            roughness: 0.8,
            double_sided: true,
            alpha_cutoff: None,
            base_color_texture: None,
            normal_texture: None,
        },
        buffers: MeshBuffers::default(),
    };
    part.buffers = buffers;
    TreeMesh {
        parts: vec![part],
        ..TreeMesh::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(
        args.next()
            .ok_or("usage: label_render <out_dir> [preset ...]")?,
    );
    let mut names: Vec<String> = args.collect();
    if names.is_empty() {
        names = DEFAULT_PRESETS.map(str::to_owned).to_vec();
    }
    std::fs::create_dir_all(&out)?;
    let mut renderer = Renderer::new();
    let options = ThumbnailOptions {
        width: 640,
        height: 640,
        azimuth: 20.0,
        ..ThumbnailOptions::default()
    };
    for name in names {
        let species = preset(&name).ok_or_else(|| format!("no preset `{name}`"))?;
        let [young, old] = species.variation.age;
        let params = GenParams::new(plant_seed(1, 0), u16::midpoint(young, old));
        let mut generator = Generator::new();
        let plant = generator.generate(&species, &params)?;
        let provenance = generator.provenance(&plant, &species)?;
        let labels = MeshLabels::from_provenance(&provenance).parts();
        let semantic = recolored(&plant.mesh, &labels, |class, _| {
            CLASS_COLORS[usize::from(class.min(3))]
        });
        let instance = recolored(&plant.mesh, &labels, instance_color);
        let cloud = sample_points(&plant.mesh, &labels, 20_000, params.mesh.seed);
        let points = point_mesh(&cloud, plant.stats.height.max(0.1) / 400.0);
        let leaf_sites: Vec<_> = plant.graph.organs().iter().map(|site| site.node).collect();
        let center = leaf_sites
            .get(leaf_sites.len() / 2)
            .map_or([0.0; 3], |node| plant.graph.position(*node).to_array());
        let near = closeup(&semantic, center, 0.25);
        let images = [
            ("semantic", semantic),
            ("instance", instance),
            ("points", points),
            ("closeup", near),
        ];
        for (suffix, mesh) in images {
            let path = out.join(format!("{name}_{suffix}.png"));
            renderer.thumbnail(&mesh, &options).write_png(&path)?;
            println!("wrote {}", path.display());
        }
        let mut counts = [0_usize; 4];
        for part in &labels {
            for &class in &part.semantic {
                counts[usize::from(class.min(3))] += 1;
            }
        }
        println!("{name}: vertices per class (trunk, branch, twig, leaf) {counts:?}");
    }
    Ok(())
}
