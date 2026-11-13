//! Properties of the meshes of hand-built skeletons: well-formed geometry, outward
//! winding, counts, determinism across thread counts, level of detail, age and export.

mod common;

use arbrume_core::graph::{ChainKind, Metamer, OrganKind, OrganSite, PlantGraph};
use arbrume_core::mesh::{PartKind, TreeMesh};
use arbrume_core::rng::{Rng, Stream};
use arbrume_core::species::Species;
use arbrume_export::{GlbOptions, write_glb};
use arbrume_mesh::synthetic::{SkeletonParams, skeleton};
use arbrume_mesh::{MeshError, MeshOptions, Mesher, mesh_counts, mesh_tree};
use common::{area, assert_well_formed, bits, bounds, part, triangles};
use glam::Vec3;

fn species(toml: &str) -> Species {
    Species::from_toml_str(toml).unwrap()
}

/// Meshes `graph` and checks the result against the counting pass.
fn checked(graph: &PlantGraph, species: &Species, options: &MeshOptions) -> TreeMesh {
    let mesh = mesh_tree(graph, species, options).unwrap();
    let counts = mesh_counts(graph, species, options).unwrap();
    let sizes = |kind| {
        part(&mesh, kind).map_or((0, 0), |part| {
            (part.buffers.positions.len(), part.buffers.triangle_count())
        })
    };
    assert_eq!(
        sizes(PartKind::Bark),
        (counts.bark_vertices, counts.bark_triangles)
    );
    assert_eq!(
        sizes(PartKind::Leaf),
        (counts.leaf_vertices, counts.leaf_triangles)
    );
    assert_eq!(mesh.triangle_count(), counts.triangles());
    mesh.parts.iter().for_each(assert_well_formed);
    mesh
}

#[test]
fn random_skeletons_give_well_formed_meshes_at_every_setting() {
    let plain = Species::default();
    let lobed = species(
        "[geometry]\nflare = 1.2\nbuttress_lobes = 5\nbuttress_depth = 0.5\n\
         [foliage]\nleaflet_pairs = 2\nshape = \"lanceolate\"",
    );
    let cards = species("[foliage]\ngeometry = \"card\"\n[geometry]\nradial_segments = [5, 9]");
    for seed in 0..6 {
        let graph = skeleton(&SkeletonParams::small(seed));
        for species in [&plain, &lobed, &cards] {
            for quality in [1.0, 0.5, 0.2] {
                let options = MeshOptions {
                    seed,
                    ..MeshOptions::with_quality(quality)
                };
                let mesh = checked(&graph, species, &options);
                assert_eq!(mesh.parts.len(), 2, "seed {seed} quality {quality}");
            }
        }
        let fine = MeshOptions {
            tolerance: 0.0002,
            ..MeshOptions::default()
        };
        checked(&graph, &lobed, &fine);
    }
}

#[test]
fn mesh_encloses_the_skeleton() {
    for seed in 0..6 {
        let graph = skeleton(&SkeletonParams::small(seed));
        let options = MeshOptions::default();
        let mesh = checked(&graph, &Species::default(), &options);
        let (low, high) = bounds(&part(&mesh, PartKind::Bark).unwrap().buffers);
        let skeleton = graph.bounds().unwrap();
        let margin = Vec3::splat(options.tolerance);
        assert!(low.cmple(skeleton.min + margin).all(), "seed {seed}");
        assert!(high.cmpge(skeleton.max - margin).all(), "seed {seed}");
        let reach = Vec3::splat(1.0);
        assert!(low.cmpge(skeleton.min - reach).all() && high.cmple(skeleton.max + reach).all());
    }
}

/// Several unbranched, gently wandering axes far apart, with varying radii. The bends
/// are small enough for the smoothed centerline to stay well inside the tube around the
/// polyline, so the polyline can stand for the axis.
fn lone_axes(seed: u64) -> PlantGraph {
    let mut graph = PlantGraph::new();
    for axis in 0..6_u32 {
        let mut rng = Rng::new(seed, axis, Stream::custom(7));
        let origin = Vec3::new(axis as f32 * 20.0, 0.0, 0.0);
        let mut direction = rng.unit_sphere();
        let mut position = origin + direction * 0.3;
        let base = rng.range(0.04, 0.2);
        let node = graph.add_root_chain(origin, Metamer::new(position, base, 0));
        let chain = graph.chain_of(node);
        let nodes = 5 + rng.below(30);
        for index in 1..nodes {
            direction = (direction + rng.unit_sphere() * 0.05).normalize();
            position += direction * rng.range(0.15, 0.5);
            let radius = base * (1.0 - 0.6 * index as f32 / nodes as f32);
            graph.extend_chain(chain, Metamer::new(position, radius, 0));
        }
    }
    graph
}

/// Distance vector from the nearest point of the polyline of `chain` to `point`.
fn from_axis(graph: &PlantGraph, chain: usize, point: Vec3) -> Vec3 {
    let view = graph
        .chain_view(graph.chain_ids().nth(chain).unwrap())
        .unwrap();
    let mut previous = view.origin;
    let mut best = Vec3::splat(f32::INFINITY);
    for &next in view.positions {
        let chord = next - previous;
        let u = ((point - previous).dot(chord) / chord.length_squared()).clamp(0.0, 1.0);
        let offset = point - (previous + chord * u);
        if offset.length_squared() < best.length_squared() {
            best = offset;
        }
        previous = next;
    }
    best
}

#[test]
fn triangles_and_normals_face_away_from_the_axis() {
    let bare = species("[foliage]\nenabled = false");
    for seed in 0..8 {
        let graph = lone_axes(seed);
        let mesh = checked(&graph, &bare, &MeshOptions::default());
        assert_eq!(mesh.parts.len(), 1);
        let buffers = &mesh.parts[0].buffers;
        for [a, b, c] in triangles(buffers) {
            let centroid = (a + b + c) / 3.0;
            let chain = (centroid.x / 20.0).round().clamp(0.0, 5.0) as usize;
            let outward = from_axis(&graph, chain, centroid);
            assert!((b - a).cross(c - a).dot(outward) > 0.0, "seed {seed}");
        }
        for (position, normal) in buffers.positions.iter().zip(&buffers.normals) {
            let position = Vec3::from(*position);
            let chain = (position.x / 20.0).round().clamp(0.0, 5.0) as usize;
            let outward = from_axis(&graph, chain, position);
            assert!(Vec3::from(*normal).dot(outward) > 0.0, "seed {seed}");
        }
    }
}

#[test]
fn output_is_bit_identical_for_any_thread_count() {
    let species = Species::default();
    for (params, quality) in [
        (SkeletonParams::medium(3), 1.0),
        (SkeletonParams::small(5), 0.4),
    ] {
        let graph = skeleton(&params);
        let sequential = MeshOptions {
            seed: 9,
            ..MeshOptions::with_quality(quality)
        };
        let parallel = MeshOptions {
            parallel: true,
            ..sequential
        };
        let reference = mesh_tree(&graph, &species, &sequential).unwrap();
        let expected = bits(&reference);
        assert!(reference.triangle_count() > 1_000);
        for threads in [1, 2, 8] {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap();
            let mesh = pool.install(|| mesh_tree(&graph, &species, &parallel).unwrap());
            assert!(bits(&mesh) == expected, "{threads} threads differ");
            assert_eq!(mesh, reference);
        }
        let again = mesh_tree(&graph, &species, &sequential).unwrap();
        assert!(bits(&again) == expected, "second run differs");
        let other_seed = MeshOptions {
            seed: 10,
            ..sequential
        };
        assert_ne!(mesh_tree(&graph, &species, &other_seed).unwrap(), reference);
    }
}

#[test]
fn reused_mesher_and_buffers_give_the_same_mesh() {
    let plain = Species::default();
    let cards = species("[foliage]\ngeometry = \"card\"\n[geometry]\nbuttress_lobes = 6");
    let bare = species("[foliage]\nenabled = false");
    let mut mesher = Mesher::new();
    let mut reused = TreeMesh::default();
    let runs = [
        (SkeletonParams::medium(1), &plain, 1.0, false),
        (SkeletonParams::small(2), &cards, 0.5, true),
        (SkeletonParams::small(3), &bare, 1.0, false),
        (SkeletonParams::small(4), &plain, 0.3, true),
        (SkeletonParams::medium(5), &cards, 0.8, false),
    ];
    for (params, species, quality, parallel) in runs {
        let graph = skeleton(&params);
        let options = MeshOptions {
            parallel,
            seed: params.seed,
            ..MeshOptions::with_quality(quality)
        };
        let fresh = mesh_tree(&graph, species, &options).unwrap();
        mesher
            .mesh_into(&graph, species, &options, &mut reused)
            .unwrap();
        assert!(bits(&reused) == bits(&fresh), "seed {}", params.seed);
        assert_eq!(reused, fresh);
        assert_eq!(mesher.mesh(&graph, species, &options).unwrap(), fresh);
        let counts = mesher.counts(&graph, species, &options).unwrap();
        assert_eq!(counts.triangles(), fresh.triangle_count());
    }
    let invalid = MeshOptions {
        leaf_fraction: 2.0,
        ..MeshOptions::default()
    };
    let before = reused.clone();
    let graph = skeleton(&SkeletonParams::small(1));
    assert!(
        mesher
            .mesh_into(&graph, &plain, &invalid, &mut reused)
            .is_err()
    );
    assert_eq!(reused, before, "a failed call leaves the output untouched");
}

#[test]
fn lower_quality_gives_fewer_triangles_and_keeps_leaf_area() {
    let species = Species::default();
    for seed in 0..4 {
        let graph = skeleton(&SkeletonParams::small(seed));
        let full = checked(&graph, &species, &MeshOptions::default());
        let full_area = area(&part(&full, PartKind::Leaf).unwrap().buffers);
        let mut previous = mesh_counts(&graph, &species, &MeshOptions::default()).unwrap();
        for quality in [0.8, 0.6, 0.45, 0.3, 0.2, 0.1] {
            let options = MeshOptions::with_quality(quality);
            let counts = mesh_counts(&graph, &species, &options).unwrap();
            assert!(counts.bark_triangles < previous.bark_triangles, "{quality}");
            assert!(counts.leaf_triangles < previous.leaf_triangles, "{quality}");
            assert!(counts.leaves < previous.leaves, "{quality}");
            previous = counts;
            let mesh = mesh_tree(&graph, &species, &options).unwrap();
            let leaf_area = area(&part(&mesh, PartKind::Leaf).unwrap().buffers);
            let change = (leaf_area / full_area - 1.0).abs();
            assert!(change < 0.12, "leaf area changed by {change} at {quality}");
        }
    }
}

#[test]
fn each_level_of_detail_control_acts_alone() {
    let species = Species::default();
    let graph = skeleton(&SkeletonParams::small(2));
    let full = mesh_counts(&graph, &species, &MeshOptions::default()).unwrap();
    let with = |options: MeshOptions| mesh_counts(&graph, &species, &options).unwrap();
    let coarse = with(MeshOptions {
        tolerance: 0.02,
        ..MeshOptions::default()
    });
    assert!(coarse.bark_triangles < full.bark_triangles && coarse.leaves == full.leaves);
    let pruned = with(MeshOptions {
        min_radius: 0.004,
        ..MeshOptions::default()
    });
    assert!(pruned.bark_triangles < full.bark_triangles && pruned.leaves == full.leaves);
    let sparse = with(MeshOptions {
        leaf_fraction: 0.25,
        ..MeshOptions::default()
    });
    assert_eq!(sparse.bark_triangles, full.bark_triangles);
    assert_eq!(sparse.leaves, full.leaves.div_ceil(4));
    let bare = with(MeshOptions {
        leaf_fraction: 0.0,
        ..MeshOptions::default()
    });
    assert_eq!((bare.leaves, bare.leaf_triangles), (0, 0));
}

#[test]
fn a_prefix_of_the_leaves_is_a_lower_level_of_detail() {
    let species = species("[foliage]\nsize_variation = 0.0");
    let graph = skeleton(&SkeletonParams::small(4));
    let full = mesh_tree(&graph, &species, &MeshOptions::default()).unwrap();
    let half = MeshOptions {
        leaf_fraction: 0.5,
        ..MeshOptions::default()
    };
    let half = mesh_tree(&graph, &species, &half).unwrap();
    let (full, half) = (
        &part(&full, PartKind::Leaf).unwrap().buffers,
        &part(&half, PartKind::Leaf).unwrap().buffers,
    );
    let kept = half.positions.len();
    assert_eq!(half.normals[..], full.normals[..kept]);
    assert_eq!(half.colors[..], full.colors[..kept]);
    assert_eq!(half.indices[..], full.indices[..half.indices.len()]);
    let mean_height = |positions: &[[f32; 3]]| {
        positions.iter().map(|p| f64::from(p[1])).sum::<f64>() / positions.len() as f64
    };
    let shift = mean_height(&half.positions) - mean_height(&full.positions);
    assert!(shift.abs() < 0.5, "the kept half is biased by {shift} m");
}

#[test]
fn age_cut_meshes_a_younger_plant() {
    let species = species("[foliage]\nretention_cycles = 3");
    let graph = skeleton(&SkeletonParams::small(1));
    let last = *graph.births().iter().max().unwrap();
    let at = |cycle| MeshOptions {
        max_cycle: Some(cycle),
        ..MeshOptions::default()
    };
    let whole = checked(&graph, &species, &MeshOptions::default());
    let grown = checked(&graph, &species, &at(last));
    assert_eq!(
        part(&grown, PartKind::Bark),
        part(&whole, PartKind::Bark),
        "a cut at the last cycle keeps every axis"
    );
    let mut previous = 0;
    for cycle in 0..=last {
        let mesh = checked(&graph, &species, &at(cycle));
        let bark = part(&mesh, PartKind::Bark).unwrap();
        assert!(bark.buffers.triangle_count() >= previous, "cycle {cycle}");
        previous = bark.buffers.triangle_count();
        let (low, high) = bounds(&bark.buffers);
        let (whole_low, whole_high) = bounds(&part(&whole, PartKind::Bark).unwrap().buffers);
        assert!(low.cmpge(whole_low - 0.2).all() && high.cmple(whole_high + 0.2).all());
    }
    let kept = |cycle| mesh_counts(&graph, &species, &at(cycle)).unwrap().leaves;
    let every = mesh_counts(&graph, &species, &MeshOptions::default()).unwrap();
    assert!(
        kept(last) < every.leaves,
        "old leaves are dropped at the cut"
    );
    assert!(kept(last) > 0 && kept(0) <= kept(last));
}

/// A straight vertical axis of radius 0.05 bearing one horizontal leaf at mid-height.
fn one_leaf() -> PlantGraph {
    let mut graph = PlantGraph::new();
    let base = graph.add_root_chain(Vec3::ZERO, Metamer::new(Vec3::Y, 0.05, 0));
    graph.extend_chain(graph.chain_of(base), Metamer::new(Vec3::Y * 2.0, 0.05, 0));
    graph.add_organ(OrganSite {
        node: base,
        kind: OrganKind::Leaf,
        direction: Vec3::X,
        normal: Vec3::Y,
        scale: 1.0,
        birth: 0,
    });
    graph
}

#[test]
fn leaf_follows_its_site_and_starts_on_the_bark() {
    let still = species(
        "[foliage]\nsize_variation = 0.0\norientation_variation_deg = 0.0\n\
         hue_variation = 0.0\nbrightness_variation = 0.0\ncurvature_deg = 0.0\nlength = 0.1",
    );
    let graph = one_leaf();
    let mesh = checked(&graph, &still, &MeshOptions::default());
    let leaf = &part(&mesh, PartKind::Leaf).unwrap().buffers;
    let (low, high) = bounds(leaf);
    assert!((low.x - 0.05).abs() < 1e-6, "leaf starts at the surface");
    assert!(
        (high.x - (0.05 + 0.1 * 1.15)).abs() < 1e-5,
        "petiole and blade"
    );
    assert!(
        low.y >= 1.0 - 1e-6 && high.y < 1.02,
        "blade is level, margins raised"
    );
    assert!(
        (low.z + high.z).abs() < 1e-6,
        "blade is symmetric about its midrib"
    );
    assert!(leaf.normals.iter().all(|normal| normal[1] > 0.9));
    assert!(
        leaf.colors
            .iter()
            .flatten()
            .all(|v| v.to_bits() == 1.0_f32.to_bits())
    );
    let material = &part(&mesh, PartKind::Leaf).unwrap().material;
    assert!(material.double_sided && material.alpha_cutoff.is_none());
    assert_eq!(material.base_color[..3], still.foliage.color);

    let varied = Species::default();
    let jittered = checked(&graph, &varied, &MeshOptions::default());
    let jittered = &part(&jittered, PartKind::Leaf).unwrap().buffers;
    let mean = jittered
        .normals
        .iter()
        .fold(Vec3::ZERO, |sum, normal| sum + Vec3::from(*normal))
        .normalize();
    let limit = (2.5 * varied.foliage.orientation_variation_deg)
        .to_radians()
        .cos();
    assert!(mean.dot(Vec3::Y) > limit && mean.dot(Vec3::Y) < 1.0);
    let shade = jittered.colors[0].map(f32::to_bits);
    let same = |color: &[f32; 4]| color.map(f32::to_bits) == shade;
    assert!(jittered.colors.iter().all(same));
}

#[test]
fn shed_branches_other_organs_and_disabled_foliage_are_left_out() {
    let mut graph = one_leaf();
    let base = graph.node_ids().next().unwrap();
    let side = graph.add_chain(
        base,
        ChainKind::Lateral,
        Metamer::new(Vec3::new(0.5, 1.2, 0.0), 0.02, 1),
    );
    let mut site = graph.organs()[0];
    site.node = side;
    graph.add_organ(site);
    site.kind = OrganKind::Fruit;
    graph.add_organ(site);
    let species = Species::default();
    let options = MeshOptions::default();
    let before = mesh_counts(&graph, &species, &options).unwrap();
    assert_eq!(before.leaves, 2);
    graph.shed_subtree(side);
    let after = mesh_counts(&graph, &species, &options).unwrap();
    assert_eq!(after.leaves, 1);
    assert!(after.bark_triangles < before.bark_triangles);
    checked(&graph, &species, &options);
    let bare = Species::from_toml_str("[foliage]\nenabled = false").unwrap();
    assert_eq!(checked(&graph, &bare, &options).parts.len(), 1);
}

#[test]
fn empty_and_non_contiguous_graphs_are_handled() {
    let species = Species::default();
    let options = MeshOptions::default();
    let empty = mesh_tree(&PlantGraph::new(), &species, &options).unwrap();
    assert_eq!(empty, TreeMesh::default());

    let mut graph = PlantGraph::new();
    let base = graph.add_root_chain(Vec3::ZERO, Metamer::new(Vec3::Y, 0.1, 0));
    let trunk = graph.chain_of(base);
    graph.add_chain(base, ChainKind::Lateral, Metamer::new(Vec3::ONE, 0.03, 0));
    graph.extend_chain(trunk, Metamer::new(Vec3::Y * 2.0, 0.08, 0));
    assert!(!graph.is_contiguous());
    assert_eq!(
        mesh_tree(&graph, &species, &options),
        Err(MeshError::NotContiguous)
    );
    assert_eq!(
        mesh_counts(&graph, &species, &options),
        Err(MeshError::NotContiguous)
    );
    graph.ensure_contiguous();
    assert_eq!(checked(&graph, &species, &options).parts.len(), 1);
    let invalid = MeshOptions {
        tolerance: -1.0,
        ..options
    };
    assert!(matches!(
        mesh_tree(&graph, &species, &invalid),
        Err(MeshError::InvalidOption {
            field: "tolerance",
            ..
        })
    ));
}

#[test]
fn bark_texture_coordinates_follow_circumference_and_arc_length() {
    let species = species("[foliage]\nenabled = false\n[geometry]\nflare = 0.0");
    let mut graph = PlantGraph::new();
    let base = graph.add_root_chain(Vec3::ZERO, Metamer::new(Vec3::Y * 3.0, 0.2, 0));
    graph.extend_chain(graph.chain_of(base), Metamer::new(Vec3::Y * 6.0, 0.1, 0));
    let mesh = checked(&graph, &species, &MeshOptions::default());
    let buffers = &mesh.parts[0].buffers;
    let [around, along] = species.bark.texture_size;
    let repeats = (std::f32::consts::TAU * 0.2 / around).round();
    let widest = buffers.uvs.iter().map(|uv| uv[0]).fold(0.0, f32::max);
    assert!((widest - repeats).abs() < 1e-5, "u spans whole repeats");
    let texel_around = std::f32::consts::TAU * 0.2 / repeats / around;
    for (position, uv) in buffers.positions.iter().zip(&buffers.uvs) {
        if position[1] <= 6.0 {
            let expected = position[1] / (texel_around * along);
            assert!(
                (uv[1] - expected).abs() < 1e-3,
                "v is arc length in texture units"
            );
        }
    }
    assert_eq!(mesh.parts[0].material.base_color[..3], species.bark.color);
    assert_eq!(buffers.colors, [[0.0_f32; 4]; 0]);
}

#[test]
fn hand_built_tree_exports_to_glb() {
    let graph = skeleton(&SkeletonParams::small(11));
    let mesh = checked(&graph, &Species::default(), &MeshOptions::default());
    let mut bytes = Vec::new();
    write_glb(&mesh, &GlbOptions::default(), &mut bytes).unwrap();
    assert_eq!(&bytes[..4], b"glTF");
    let declared = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    assert_eq!(declared as usize, bytes.len());
    let json_length = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    let json = std::str::from_utf8(&bytes[20..20 + json_length]).unwrap();
    for needle in ["\"bark\"", "\"leaf\"", "COLOR_0", "TEXCOORD_0", "NORMAL"] {
        assert!(json.contains(needle), "{needle} missing from the glb");
    }
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("hand_built_tree.glb");
    std::fs::write(&path, &bytes).unwrap();
}
