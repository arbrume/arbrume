//! Tier B junctions: the bark is one closed surface, and what rides on it (labels, wind,
//! texture coordinates, levels of detail) stays continuous across junctions.

use arbrume_core::graph::{ChainId, ChainKind, Metamer, NodeId, PlantGraph};
use arbrume_core::mesh::{MeshBuffers, PartKind, TreeMesh, WindTable};
use arbrume_core::species::Species;
use arbrume_mesh::synthetic::{SkeletonParams, skeleton};
use arbrume_mesh::topology::{SurfaceReport, check_surface};
use arbrume_mesh::{Junctions, MeshOptions, Mesher, mesh_counts, mesh_provenance, mesh_tree};
use glam::Vec3;
use std::collections::HashMap;

/// One straight lateral: node of the trunk, azimuth and elevation in degrees, radius.
type Lateral = (usize, f32, f32, f32);

/// A straight vertical trunk of `nodes` nodes 0.2 m apart and base radius `radius`,
/// tapering, with one straight lateral of four internodes per entry of `laterals`.
fn plant(nodes: usize, radius: f32, laterals: &[Lateral]) -> PlantGraph {
    let mut graph = PlantGraph::default();
    let first = graph.add_root_chain(Vec3::ZERO, Metamer::new(Vec3::Y * 0.2, radius, 0));
    let trunk = graph.chain_of(first);
    let mut trunk_nodes = vec![first];
    for i in 2..=nodes {
        let taper = radius * (1.0 - 0.3 * i as f32 / nodes as f32);
        let node = graph.extend_chain(trunk, Metamer::new(Vec3::Y * 0.2 * i as f32, taper, 0));
        trunk_nodes.push(node);
    }
    for &(node, azimuth, elevation, child_radius) in laterals {
        let attach: NodeId = trunk_nodes[node];
        let (a, e) = (azimuth.to_radians(), elevation.to_radians());
        let direction = Vec3::new(a.cos() * e.cos(), e.sin(), a.sin() * e.cos());
        let origin = graph.position(attach);
        let metamer = |k: f32| Metamer::new(origin + direction * 0.25 * k, child_radius, 1);
        let start = graph.add_chain(attach, ChainKind::Lateral, metamer(1.0));
        let chain = graph.chain_of(start);
        for k in 2..=4 {
            graph.extend_chain(chain, metamer(k as f32));
        }
    }
    graph
}

fn tier_b() -> MeshOptions {
    MeshOptions {
        junctions: Junctions::TierB,
        ..MeshOptions::default()
    }
}

fn bark(mesh: &TreeMesh) -> &MeshBuffers {
    let part = mesh.parts.iter().find(|part| part.kind == PartKind::Bark);
    &part.unwrap().buffers
}

/// Every number of the bark buffers as raw bits, for exact comparison.
fn bits(mesh: &TreeMesh) -> Vec<u32> {
    let bark = bark(mesh);
    let floats = bark.positions.iter().chain(&bark.normals).flatten();
    let uvs = bark.uvs.iter().flatten();
    let mut out: Vec<u32> = floats.chain(uvs).map(|v| v.to_bits()).collect();
    out.extend(&bark.indices);
    out
}

fn bark_report(graph: &PlantGraph, options: &MeshOptions) -> SurfaceReport {
    let mesh = mesh_tree(graph, &Species::default(), options).unwrap();
    let bark = bark(&mesh);
    check_surface(&bark.positions, &bark.indices, true)
}

#[test]
fn single_laterals_are_watertight() {
    let cases = [
        (0.0, 0.04),
        (30.0, 0.04),
        (-20.0, 0.02),
        (40.0, 0.08),
        (10.0, 0.005),
        (0.0, 0.002),
        (30.0, 0.0002),
        (-10.0, 0.0001),
    ];
    for (elevation, child) in cases {
        for trunk in [0.1_f32, 0.01] {
            let laterals = [(5, 20.0, elevation, f32::min(child, 0.8 * trunk))];
            let report = bark_report(&plant(10, trunk, &laterals), &tier_b());
            assert!(
                report.is_watertight(),
                "trunk {trunk} {elevation} {child}: {report:?}"
            );
        }
    }
}

#[test]
fn whorls_forks_and_tips_are_watertight() {
    let whorl: Vec<Lateral> = (0..5).map(|k| (4, 72.0 * k as f32, 10.0, 0.03)).collect();
    let stacked = [
        (3, 0.0, 20.0, 0.03),
        (4, 10.0, 20.0, 0.03),
        (5, 20.0, 20.0, 0.03),
    ];
    let fork = [(6, 45.0, 50.0, 0.065)];
    let tip = [(9, 90.0, 30.0, 0.04), (9, 270.0, 30.0, 0.03)];
    let first = [(0, 0.0, 0.0, 0.05)];
    for laterals in [&whorl[..], &stacked, &fork, &tip, &first] {
        let report = bark_report(&plant(10, 0.1, laterals), &tier_b());
        assert!(report.is_watertight(), "{laterals:?}: {report:?}");
    }
}

/// Every triangle faces the way the sum of its vertex normals points, and the normals
/// are unit vectors. On a parent of very few sides a stitch triangle can lean away from
/// the normal of one rim corner, which is that of the parent there, so the normals are
/// not checked one by one. The grid vertices inside a hole are left unused.
fn assert_oriented(bark: &MeshBuffers) {
    for triangle in bark.indices.chunks(3) {
        let [a, b, c] = [0, 1, 2].map(|k| Vec3::from(bark.positions[triangle[k] as usize]));
        let normals: Vec3 = triangle
            .iter()
            .map(|&v| Vec3::from(bark.normals[v as usize]))
            .sum();
        assert!(
            (b - a).cross(c - a).dot(normals) > 0.0,
            "{triangle:?} faces away"
        );
    }
    let unit = |n: &[f32; 3]| (Vec3::from(*n).length() - 1.0).abs() < 1e-4;
    assert!(bark.normals.iter().all(unit));
}

#[test]
fn tier_b_is_deterministic_and_counted_in_advance() {
    let graph = skeleton(&SkeletonParams::medium(3));
    let species = Species::default();
    let serial = mesh_tree(&graph, &species, &tier_b()).unwrap();
    let parallel_options = MeshOptions {
        parallel: true,
        ..tier_b()
    };
    let parallel = mesh_tree(&graph, &species, &parallel_options).unwrap();
    assert_eq!(bits(&serial), bits(&parallel));
    let mut mesher = Mesher::new();
    mesher
        .mesh(&graph, &species, &MeshOptions::default())
        .unwrap();
    let reused = mesher.mesh(&graph, &species, &tier_b()).unwrap();
    assert_eq!(bits(&serial), bits(&reused));
    let counts = mesh_counts(&graph, &species, &tier_b()).unwrap();
    let bark = bark(&serial);
    assert_eq!(counts.bark_vertices, bark.positions.len());
    assert_eq!(counts.bark_triangles, bark.indices.len() / 3);
    assert_oriented(bark);
    let report = check_surface(&bark.positions, &bark.indices, false);
    assert!(
        report.is_closed_manifold() && report.components == 1,
        "{report:?}"
    );
}

#[test]
fn tier_a_child_tubes_stay_open() {
    let graph = skeleton(&SkeletonParams::medium(5));
    let tier_a = mesh_tree(&graph, &Species::default(), &MeshOptions::default()).unwrap();
    let report = check_surface(&bark(&tier_a).positions, &bark(&tier_a).indices, false);
    assert!(report.boundary_edges > 0);
}

/// Chain of each bark vertex, from the provenance of the mesh.
fn vertex_chains(graph: &PlantGraph, options: &MeshOptions, vertices: usize) -> Vec<u32> {
    let provenance = mesh_provenance(graph, &Species::default(), options).unwrap();
    let (_, runs) = provenance
        .parts
        .iter()
        .find(|(kind, _)| *kind == PartKind::Bark)
        .unwrap();
    let mut chains = vec![u32::MAX; vertices];
    for run in runs {
        chains[run.start as usize..(run.start + run.len) as usize].fill(run.instance);
    }
    chains
}

/// Groups of vertices with exactly the same position.
fn coincident(positions: &[[f32; 3]]) -> Vec<Vec<usize>> {
    let mut groups: HashMap<[u32; 3], Vec<usize>> = HashMap::new();
    for (index, position) in positions.iter().enumerate() {
        groups
            .entry(position.map(f32::to_bits))
            .or_default()
            .push(index);
    }
    groups
        .into_values()
        .filter(|group| group.len() > 1)
        .collect()
}

/// Checks two coincident vertices of chains `a` and `b`: they belong to the same chain
/// or to a parent and its child, and when they also share their normal (a copy of a
/// parent vertex on the rim of a hole) their texture coordinates agree up to whole
/// repeats around.
fn check_copy(bark: &MeshBuffers, (first, second): (usize, usize), parents: (bool, bool)) {
    let (ua, ub) = (bark.uvs[first], bark.uvs[second]);
    assert!(parents.0 || parents.1, "unrelated chains share a vertex");
    let bits = |normal: [f32; 3]| normal.map(f32::to_bits);
    if bits(bark.normals[first]) == bits(bark.normals[second]) {
        let around = (ua[0] - ub[0]).abs();
        assert!((around - around.round()).abs() < 1e-4, "{ua:?} {ub:?}");
        assert!((ua[1] - ub[1]).abs() < 1e-5, "{ua:?} {ub:?}");
    }
}

#[test]
fn junction_vertices_belong_to_the_child_and_copy_the_parent() {
    let graph = skeleton(&SkeletonParams::medium(9));
    let options = MeshOptions {
        wind: true,
        ..tier_b()
    };
    let mesh = mesh_tree(&graph, &Species::default(), &options).unwrap();
    let bark = bark(&mesh);
    let chains = vertex_chains(&graph, &options, bark.positions.len());
    assert!(chains.iter().all(|&chain| chain != u32::MAX));
    for triangle in bark.indices.chunks(3) {
        let chain = chains[triangle[0] as usize];
        assert!(triangle.iter().all(|&v| chains[v as usize] == chain));
    }
    let parent = |chain: u32| {
        let attach = graph.chain(ChainId(chain)).attach;
        (!attach.is_none()).then(|| graph.chain_of(attach).0)
    };
    for group in coincident(&bark.positions) {
        let wind = bark.wind[group[0]].map(f32::to_bits);
        let same = |&v: &usize| bark.wind[v].map(f32::to_bits) == wind;
        assert!(group.iter().all(same), "wind differs");
        for pair in group.windows(2) {
            let (a, b) = (chains[pair[0]], chains[pair[1]]);
            if a != b {
                let related = (parent(a) == Some(b), parent(b) == Some(a));
                check_copy(bark, (pair[0], pair[1]), related);
            }
        }
    }
}

/// Offset of a vertex by the demonstration shader of the wind documentation at `time`,
/// with a phase per branch.
fn wind_offset(table: &WindTable, vertex: &[f32; 6], time: f32) -> Vec3 {
    let wind = Vec3::new(1.0, 0.0, 0.3).normalize();
    let own = &table.branches[vertex[4] as usize];
    let mut offset = Vec3::ZERO;
    for k in 0..=usize::from(own.level) {
        let branch = &table.branches[own.lineage[k] as usize];
        let x = vertex[k];
        let u = branch.shape[0] * x * x + branch.shape[1] * x.powi(4);
        let tangent = Vec3::from(branch.tangent);
        let across = (wind - tangent * wind.dot(tangent)).normalize_or_zero();
        let phase = own.lineage[k] as f32 * 0.7;
        let frequency = (0.35 + 0.45 * k as f32) * std::f32::consts::TAU;
        let sway = 0.6 + 0.4 * (time * frequency + phase).sin();
        offset += across * 0.03 * branch.length / (1.0 + k as f32) * u * sway;
    }
    offset
}

/// Largest change of length in meters, over a few times, of the bark edges between
/// vertices of different branches (stitches and collars) and of the other edges. The
/// first is split in two: junctions of a child whose wind level is that of its parent,
/// the deepest level, which pivots on its own base, and the others.
fn edge_stretch(mesh: &TreeMesh) -> [f32; 3] {
    let (bark, table) = (bark(mesh), mesh.wind.as_ref().unwrap());
    let mut largest = [0.0_f32; 3];
    let capped = |a: usize, b: usize| {
        let (a, b) = (&table.branches[a], &table.branches[b]);
        a.level == b.level
    };
    for time in [0.0, 0.37, 1.1, 2.9] {
        let moved: Vec<Vec3> = bark
            .positions
            .iter()
            .zip(&bark.wind)
            .map(|(&p, w)| Vec3::from(p) + wind_offset(table, w, time))
            .collect();
        for triangle in bark.indices.chunks(3) {
            for side in 0..3 {
                let (a, b) = (triangle[side] as usize, triangle[(side + 1) % 3] as usize);
                let rest = Vec3::from(bark.positions[a]).distance(Vec3::from(bark.positions[b]));
                let stretch = (moved[a].distance(moved[b]) - rest).abs();
                let (wa, wb) = (bark.wind[a][4] as usize, bark.wind[b][4] as usize);
                let kind = if wa == wb {
                    2
                } else {
                    usize::from(capped(wa, wb))
                };
                largest[kind] = largest[kind].max(stretch);
            }
        }
    }
    largest
}

#[test]
fn wind_does_not_tear_junctions() {
    let graph = skeleton(&SkeletonParams::medium(4));
    let options = MeshOptions {
        wind: true,
        ..tier_b()
    };
    let mesh = mesh_tree(&graph, &Species::default(), &options).unwrap();
    let (bark, table) = (bark(&mesh), mesh.wind.as_ref().unwrap());
    for group in coincident(&bark.positions) {
        for time in [0.0, 0.37, 1.1, 2.9] {
            let first = wind_offset(table, &bark.wind[group[0]], time);
            assert!(
                group
                    .iter()
                    .all(|&v| wind_offset(table, &bark.wind[v], time) == first)
            );
        }
    }
    let [junction, capped, other] = edge_stretch(&mesh);
    eprintln!(
        "largest change of edge length (m): {junction} at junctions, {capped} at the deepest level, {other} elsewhere"
    );
    assert!(junction <= other, "{junction} {other}");
}

#[test]
fn lower_levels_of_detail_have_fewer_triangles() {
    let graph = skeleton(&SkeletonParams::medium(6));
    let species = Species::default();
    let mut previous = usize::MAX;
    for quality in [1.0, 0.7, 0.5, 0.35, 0.25] {
        let options = MeshOptions {
            junctions: Junctions::TierB,
            ..MeshOptions::with_quality(quality)
        };
        let counts = mesh_counts(&graph, &species, &options).unwrap();
        assert!(counts.bark_triangles < previous, "quality {quality}");
        previous = counts.bark_triangles;
        let report = bark_report(&graph, &options);
        assert!(
            report.is_closed_manifold() && report.components == 1,
            "{report:?}"
        );
    }
}
