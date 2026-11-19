//! Level-of-detail chains, wind data and billboards on presets.
//!
//! When `ARBRUME_GLB_OUT` names a directory, the files written here are also saved there
//! so `tools/validate_glb.mjs` can run the Khronos validator on them.

use arbrume_core::graph::{ChainId, PlantGraph};
use arbrume_core::mesh::{PartKind, TreeMesh, WindTable};
use arbrume_core::species::Species;
use arbrume_export::{
    ExportOptions, GlbLevel, GlbOptions, SkinOptions, WindLayout, write_glb_levels,
};
use arbrume_gen::{
    BillboardMethod, BillboardOptions, GenParams, Generator, LodKind, LodMesh, LodSettings, Plant,
    preset,
};
use arbrume_mesh::{LodOptions, MeshOptions, Mesher};
use arbrume_raster::{Renderer, Texture, ThumbnailOptions};
use glam::Vec3;

/// Presets and ages covering a broadleaf, a conifer with twigs and a shrub.
const PLANTS: &[(&str, u16)] = &[("broadleaf", 10), ("excurrent_conifer", 12), ("shrub", 6)];

fn grow(name: &str, age: u16) -> (Species, Plant) {
    let species = preset(name).unwrap();
    let plant = Generator::new()
        .generate(&species, &GenParams::new(3, age))
        .unwrap();
    (species, plant)
}

/// Small atlases keep the tests fast in debug builds.
fn test_billboard(method: BillboardMethod) -> BillboardOptions {
    BillboardOptions {
        method,
        atlas_size: 256,
        supersampling: 2,
        ..BillboardOptions::default()
    }
}

fn chain(species: &Species, plant: &Plant, billboard: Option<BillboardMethod>) -> Vec<LodMesh> {
    let settings = LodSettings {
        lod: LodOptions {
            levels: 4,
            ..LodOptions::default()
        },
        billboard: billboard.map(test_billboard),
        billboard_source: 1,
        wind: true,
        leaf_instances: false,
    };
    Generator::new()
        .lod_chain(plant, species, &settings)
        .unwrap()
}

#[test]
fn triangle_counts_fall_within_their_budgets() {
    for &(name, age) in PLANTS {
        let (species, plant) = grow(name, age);
        let levels = chain(&species, &plant, None);
        assert_eq!(levels.len(), 4, "{name}");
        assert_eq!(levels[0].triangles, plant.mesh.triangle_count(), "{name}");
        for pair in levels.windows(2) {
            assert!(
                pair[1].triangles < pair[0].triangles,
                "{name}: not decreasing"
            );
            assert!(
                pair[1].budget < pair[0].budget,
                "{name}: budgets not decreasing"
            );
            assert!(
                pair[1].max_screen_height < pair[0].max_screen_height,
                "{name}"
            );
        }
        for (k, level) in levels.iter().enumerate() {
            assert!(
                level.triangles <= level.budget,
                "{name} level {k}: {} > {}",
                level.triangles,
                level.budget
            );
            assert_eq!(level.mesh.triangle_count(), level.triangles);
        }
    }
}

/// Distance from `point` to the surface of chain `chain` of `graph`: to its centerline
/// minus the radius there, over its nodes born by `max_cycle`.
fn distance_to_axis(graph: &PlantGraph, chain: u32, point: Vec3) -> f32 {
    let view = graph.chain_view(ChainId(chain)).unwrap();
    let mut best = f32::INFINITY;
    let mut previous = (view.origin, view.radii[0]);
    for (&p, &r) in view.positions.iter().zip(view.radii) {
        let span = p - previous.0;
        let t = ((point - previous.0).dot(span) / span.length_squared().max(1e-12)).clamp(0.0, 1.0);
        let radius = previous.1 + (r - previous.1) * t;
        let distance = (point.distance(previous.0 + span * t) - radius).abs();
        best = best.min(distance);
        previous = (p, r);
    }
    best
}

/// Chain ids of the meshed axes of a mesh with wind data.
fn kept_chains(table: &WindTable) -> Vec<u32> {
    table.branches.iter().map(|branch| branch.chain).collect()
}

#[test]
fn no_leaf_floats_at_any_level() {
    for &(name, age) in PLANTS {
        let (species, plant) = grow(name, age);
        for level in chain(&species, &plant, None) {
            let options = level.options;
            let kept = kept_chains(level.mesh.wind.as_ref().unwrap());
            let leaves = Mesher::new()
                .leaf_instances(&plant.graph, &species, &options)
                .unwrap();
            assert!(!leaves.instances.is_empty(), "{name}");
            for leaf in &leaves.instances {
                assert!(kept.contains(&leaf.chain), "{name}: leaf on a dropped axis");
                let gap = distance_to_axis(&plant.graph, leaf.chain, Vec3::from(leaf.anchor));
                assert!(
                    gap <= options.tolerance + 1e-4,
                    "{name}: leaf {gap} m off its axis"
                );
            }
        }
    }
}

#[test]
fn reattachment_is_needed_without_the_lod_options() {
    let (species, plant) = grow("broadleaf", 10);
    let options = MeshOptions {
        min_radius: 0.01,
        ..plant.params.mesh
    };
    let leaves = Mesher::new()
        .leaf_instances(&plant.graph, &species, &options)
        .unwrap();
    let floating = leaves
        .instances
        .iter()
        .filter(|leaf| plant.graph.chain_view(ChainId(leaf.chain)).unwrap().radii[0] < 0.01)
        .count();
    assert!(
        floating > 0,
        "the radius cut should leave orphan leaves by default"
    );
}

/// Checks the wind data of every vertex of `mesh` and the lineage of its branches.
fn check_wind(name: &str, mesh: &TreeMesh, graph: &PlantGraph) {
    let table = mesh.wind.as_ref().expect("wind table");
    let branches = &table.branches;
    for part in &mesh.parts {
        assert_eq!(
            part.buffers.wind.len(),
            part.buffers.positions.len(),
            "{name}"
        );
        for wind in &part.buffers.wind {
            assert!(
                wind[..4].iter().all(|x| (0.0..=1.0).contains(x)),
                "{name}: {wind:?}"
            );
            assert!(
                (wind[4] as usize) < branches.len() && wind[4].fract() == 0.0,
                "{name}"
            );
            assert!((0.0..=1.0).contains(&wind[5]), "{name}");
            let level = usize::from(branches[wind[4] as usize].level);
            assert!(
                wind[level + 1..4].iter().all(|&x| x == 0.0),
                "{name}: above own level"
            );
            if part.kind == PartKind::Bark {
                assert_eq!(wind[5], 0.0, "{name}: bark flutters");
            }
        }
    }
    for branch in branches {
        let Some(parent) = branch.parent.map(|p| &branches[p as usize]) else {
            assert_eq!(branch.level, 0);
            continue;
        };
        let shared = usize::from(parent.level);
        assert_eq!(
            branch.lineage_x[..shared],
            parent.lineage_x[..shared],
            "{name}"
        );
        if branch.level > parent.level {
            let x = branch.lineage_x[shared];
            let along = point_at(graph, parent.chain, x * parent.length);
            let gap = along.distance(Vec3::from(branch.pivot));
            assert!(
                gap <= parent.root_radius + 1e-3,
                "{name}: junction {gap} m off"
            );
        }
    }
}

/// The point of chain `chain` at arc length `arc` along its node polyline.
fn point_at(graph: &PlantGraph, chain: u32, arc: f32) -> Vec3 {
    let view = graph.chain_view(ChainId(chain)).unwrap();
    let mut previous = view.origin;
    let mut left = arc;
    for &p in view.positions {
        let span = p.distance(previous);
        if left <= span {
            return previous.lerp(p, left / span.max(1e-12));
        }
        left -= span;
        previous = p;
    }
    previous
}

#[test]
fn wind_data_is_in_range_and_continuous_at_junctions() {
    for &(name, age) in PLANTS {
        let (species, plant) = grow(name, age);
        for level in chain(&species, &plant, None) {
            check_wind(name, &level.mesh, &plant.graph);
        }
    }
}

#[test]
fn child_bark_starts_where_its_parent_carries_it() {
    let (species, plant) = grow("broadleaf", 10);
    let levels = chain(&species, &plant, None);
    let mesh = &levels[0].mesh;
    let table = mesh.wind.as_ref().unwrap();
    let bark = &mesh.parts[0].buffers;
    for wind in &bark.wind {
        let branch = &table.branches[wind[4] as usize];
        let level = usize::from(branch.level);
        assert_eq!(wind[..level], branch.lineage_x[..level]);
    }
}

#[test]
fn billboard_atlas_covers_the_plant() {
    for &(name, age) in PLANTS {
        let (species, plant) = grow(name, age);
        for method in [BillboardMethod::Cloud, BillboardMethod::Cross] {
            let levels = chain(&species, &plant, Some(method));
            let billboard = levels.last().unwrap();
            assert_eq!(billboard.kind, LodKind::Billboard);
            let part = &billboard.mesh.parts[0];
            let bytes = part.material.base_color_texture.as_ref().unwrap();
            let texture = Texture::from_png(bytes, true).unwrap();
            for quad in part.buffers.uvs.as_chunks::<4>().0 {
                let covered = texels_in(&texture, quad).filter(|t| t.w > 0.0).count();
                assert!(covered > 0, "{name} {method:?}: an empty plane");
            }
            let overlap = silhouette_overlap(&levels[0].mesh, &billboard.mesh);
            assert!(
                overlap > 0.6,
                "{name} {method:?}: billboard covers {overlap} of level 0"
            );
        }
    }
}

/// The texels of `texture` inside the texture rectangle of a quad.
fn texels_in<'a>(texture: &'a Texture, quad: &[[f32; 2]]) -> impl Iterator<Item = glam::Vec4> + 'a {
    let (w, h) = (texture.width as f32, texture.height as f32);
    let u = quad.iter().map(|uv| uv[0]);
    let v = quad.iter().map(|uv| uv[1]);
    let (u0, u1) = (u.clone().fold(1.0, f32::min) * w, u.fold(0.0, f32::max) * w);
    let (v0, v1) = (v.clone().fold(1.0, f32::min) * h, v.fold(0.0, f32::max) * h);
    (v0 as usize..v1 as usize).flat_map(move |y| {
        (u0 as usize..u1 as usize).map(move |x| texture.texels[y * texture.width as usize + x])
    })
}

/// Fraction of the pixels covered by `reference` that `other` covers too, seen from the
/// side at a small size.
fn silhouette_overlap(reference: &TreeMesh, other: &TreeMesh) -> f32 {
    let options = ThumbnailOptions {
        width: 96,
        height: 96,
        azimuth: 20.0,
        ground: false,
        ..ThumbnailOptions::default()
    };
    let scene = options.scene(reference);
    let mut renderer = Renderer::new();
    let a = renderer.render(reference, &scene).coverage;
    let b = renderer.render(other, &scene).coverage;
    let covered = a.iter().filter(|&&c| c > 0.0).count().max(1);
    let both = a
        .iter()
        .zip(&b)
        .filter(|&(&x, &y)| x > 0.0 && y > 0.0)
        .count();
    both as f32 / covered as f32
}

/// The bytes of a glb of the whole chain, with wind, skin and billboard.
fn chain_glb(levels: &[LodMesh], export: &ExportOptions) -> Vec<u8> {
    let glb: Vec<GlbLevel> = levels
        .iter()
        .map(|level| GlbLevel {
            mesh: &level.mesh,
            extras: None,
            extra: &[],
            leaves: None,
        })
        .collect();
    let mut bytes = Vec::new();
    write_glb_levels(&glb, &GlbOptions::default(), export, &mut bytes).unwrap();
    bytes
}

fn save(name: &str, bytes: &[u8]) {
    if let Some(dir) = std::env::var_os("ARBRUME_GLB_OUT") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join(name), bytes).unwrap();
    }
}

#[test]
fn chains_are_identical_whatever_the_thread_count() {
    let (species, plant) = grow("excurrent_conifer", 12);
    let run = |threads: usize, parallel: bool| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| {
            let mut plant = plant.clone();
            plant.params.mesh.parallel = parallel;
            let levels = chain(&species, &plant, Some(BillboardMethod::Cloud));
            let export = ExportOptions {
                wind: WindLayout::TexCoord,
                skin: Some(SkinOptions::default()),
            };
            chain_glb(&levels, &export)
        })
    };
    let reference = run(1, false);
    assert_eq!(run(4, true), reference);
    assert_eq!(run(3, false), reference);
    save("lod_chain_skin.glb", &reference);
}

#[test]
fn custom_wind_layout_and_cross_billboard_export() {
    let (species, plant) = grow("shrub", 6);
    let levels = chain(&species, &plant, Some(BillboardMethod::Cross));
    let export = ExportOptions {
        wind: WindLayout::Custom,
        skin: None,
    };
    let bytes = chain_glb(&levels, &export);
    let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
    let names: Vec<String> = gltf
        .nodes()
        .filter_map(|n| n.name().map(str::to_owned))
        .collect();
    for k in 0..levels.len() {
        assert!(names.contains(&format!("tree_LOD{k}")), "{names:?}");
    }
    let mesh = gltf.meshes().next().unwrap();
    let primitive = mesh.primitives().next().unwrap();
    let semantics: Vec<String> = primitive.attributes().map(|(s, _)| s.to_string()).collect();
    assert!(semantics.iter().any(|s| s == "_WIND_X"), "{semantics:?}");
    assert!(semantics.iter().any(|s| s == "_WIND_DATA"), "{semantics:?}");
    save("lod_chain_custom_cross.glb", &bytes);
}

#[test]
fn leaf_instances_rebuild_the_leaf_part() {
    for &(name, age) in PLANTS {
        let (species, plant) = grow(name, age);
        let settings = LodSettings {
            leaf_instances: true,
            ..LodSettings::default()
        };
        let levels = Generator::new()
            .lod_chain(&plant, &species, &settings)
            .unwrap();
        let level = &levels[0];
        let leaves = level.leaves.as_ref().unwrap();
        let part = level
            .mesh
            .parts
            .iter()
            .find(|p| p.kind == PartKind::Leaf)
            .unwrap();
        let mut at = 0;
        for leaf in &leaves.instances {
            let template = &leaves.templates[leaf.variant as usize];
            let [x, y, z] = leaf.axes.map(Vec3::from);
            for local in &template.positions {
                let local = Vec3::from(*local);
                let expected =
                    Vec3::from(leaf.anchor) + (x * local.x + y * local.y + z * local.z) * leaf.size;
                let got = Vec3::from(part.buffers.positions[at]);
                assert!(got.distance(expected) < 1e-5, "{name}: vertex {at}");
                at += 1;
            }
        }
        assert_eq!(at, part.buffers.positions.len(), "{name}");
    }
}
