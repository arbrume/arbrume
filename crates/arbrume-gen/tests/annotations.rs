//! ML annotations of generated plants: skeleton round trip, depth-first layout, labels,
//! point samples and determinism.

use arbrume_core::graph::PlantGraph;
use arbrume_core::mesh::{PartKind, SemanticClass, TreeMesh};
use arbrume_core::species::Species;
use arbrume_export::annotations::{
    Annotations, BranchTable, MeshLabels, PointCloud, Skeleton, sample_points,
};
use arbrume_gen::{GenParams, Generator, Plant, preset};
use glam::{DVec3, Vec3};

/// A plant of `species` with the labels of its mesh.
fn labelled(species: &Species, params: &GenParams) -> (Plant, MeshLabels) {
    let mut generator = Generator::new();
    let plant = generator.generate(species, params).unwrap();
    let provenance = generator.provenance(&plant, species).unwrap();
    (plant, MeshLabels::from_provenance(&provenance))
}

fn annotations(plant: &Plant, labels: &MeshLabels) -> Annotations {
    Annotations {
        skeleton: Some(Skeleton::from_graph(&plant.graph)),
        labels: Some(labels.clone()),
        branches: Some(BranchTable::from_graph(&plant.graph, 6)),
        extras: Some(serde_json::json!({ "seed": plant.params.seed })),
    }
}

const PRESETS: [&str; 5] = [
    "broadleaf",
    "conifer",
    "thicket_shrub",
    "excurrent_conifer",
    "palm_like",
];

#[test]
fn the_sidecar_round_trips_and_rebuilds_the_graph() {
    for name in PRESETS {
        let species = preset(name).unwrap();
        let (plant, labels) = labelled(&species, &GenParams::new(5, 12).with_quality(0.4));
        let written = annotations(&plant, &labels);
        let mut bytes = Vec::new();
        written.write_npz(&mut bytes).unwrap();
        let read = Annotations::read_npz(&bytes).unwrap();
        assert_eq!(read, written, "{name}");
        let skeleton = read.skeleton.unwrap();
        let rebuilt = skeleton.to_graph().unwrap();
        rebuilt.check_invariants().unwrap();
        let mut original = plant.graph.clone();
        original.compact();
        assert_eq!(rebuilt.positions(), original.positions(), "{name}");
        assert_eq!(rebuilt.radii(), original.radii(), "{name}");
        assert_eq!(rebuilt.parents(), original.parents(), "{name}");
        assert_eq!(rebuilt.node_chains(), original.node_chains(), "{name}");
        assert_eq!(rebuilt.births(), original.births(), "{name}");
        assert_eq!(rebuilt.organs(), original.organs(), "{name}");
        let shed = |g: &PlantGraph| g.flags().iter().map(|f| f.is_shed()).collect::<Vec<_>>();
        assert_eq!(shed(&rebuilt), shed(&original), "{name}");
        assert_eq!(Skeleton::from_graph(&rebuilt), skeleton, "{name}");
    }
}

/// Subtree sizes of a forest given by parent rows, parents before children.
fn subtree_sizes(parents: &[i32]) -> Vec<usize> {
    let mut sizes = vec![1_usize; parents.len()];
    for row in (0..parents.len()).rev() {
        if let Ok(parent) = usize::try_from(parents[row]) {
            sizes[parent] += sizes[row];
        }
    }
    sizes
}

/// Asserts that every subtree is the contiguous run of rows starting at its root.
fn assert_contiguous_subtrees(parents: &[i32]) {
    let sizes = subtree_sizes(parents);
    for (row, &parent) in parents.iter().enumerate() {
        let Ok(parent) = usize::try_from(parent) else {
            continue;
        };
        assert!(parent < row, "row {row}: parent {parent} comes after it");
        let end = parent + sizes[parent];
        assert!(
            row < end && row + sizes[row] <= end,
            "row {row} leaves its parent's run"
        );
    }
}

#[test]
fn depth_first_rows_keep_every_subtree_contiguous() {
    for name in PRESETS {
        let species = preset(name).unwrap();
        let (plant, _) = labelled(&species, &GenParams::new(2, 14).with_quality(0.4));
        let skeleton = Skeleton::from_graph(&plant.graph);
        assert_contiguous_subtrees(&skeleton.node_parent);
        for row in 0..skeleton.chain_id.len() {
            let (first, last) = (skeleton.chain_first[row], skeleton.chain_last[row]);
            let id = skeleton.chain_id[row];
            let members = skeleton.node_chain.iter().filter(|&&c| c == id).count();
            assert_eq!((last - first + 1) as usize, members, "{name} chain {id}");
        }
        let starts = skeleton.chain_first.windows(2).all(|w| w[0] < w[1]);
        assert!(starts, "{name}: chain rows are not in depth-first order");
        let branches = BranchTable::from_graph(&plant.graph, 4);
        assert_contiguous_subtrees(&branches.parent);
        assert_eq!(branches.polyline.len(), branches.len() * 4);
        for row in 0..branches.len() {
            let attach = branches.attach[row];
            assert!((0.0..=1.0).contains(&attach), "{name}: attach {attach}");
            let (start, end) = (branches.start[row], branches.end[row]);
            assert_eq!(branches.polyline[row * 4], start);
            assert_eq!(branches.polyline[row * 4 + 3], end);
        }
    }
}

#[test]
fn labels_cover_every_vertex_with_valid_classes_and_instances() {
    let mut coarse = GenParams::new(3, 14).with_quality(0.15);
    coarse.mesh.twig_leaf_fraction = 0.5;
    let fine = GenParams::new(3, 10).with_quality(0.5);
    let names = ["broadleaf", "conifer", "sapling", "palm_like", "fern_like"];
    let cases = names
        .iter()
        .flat_map(|name| [(name, &fine), (name, &coarse)]);
    for (name, params) in cases {
        let species = preset(name).unwrap();
        let (plant, labels) = labelled(&species, params);
        let skeleton = Skeleton::from_graph(&plant.graph);
        let vertices: usize = plant
            .mesh
            .parts
            .iter()
            .map(|p| p.buffers.positions.len())
            .sum();
        assert_eq!(labels.semantic.len(), vertices, "{name}");
        for (index, part) in plant.mesh.parts.iter().enumerate() {
            let own = labels.part(index);
            assert_eq!(own.semantic.len(), part.buffers.positions.len(), "{name}");
            for triangle in part.buffers.indices.as_chunks::<3>().0 {
                let label = |v: u32| (own.semantic[v as usize], own.instance[v as usize]);
                let first = label(triangle[0]);
                assert!(triangle.iter().all(|&v| label(v) == first), "{name}");
            }
            for (&class, &instance) in own.semantic.iter().zip(&own.instance) {
                let class = SemanticClass::from_u8(class).expect("valid class");
                check_instance(&plant.graph, &skeleton, part.kind, class, instance);
            }
        }
    }
}

/// Asserts that `instance` exists and agrees with `class` and the part `kind`.
fn check_instance(
    graph: &PlantGraph,
    skeleton: &Skeleton,
    kind: PartKind,
    class: SemanticClass,
    instance: u32,
) {
    match class {
        SemanticClass::Trunk | SemanticClass::Branch => {
            assert_eq!(kind, PartKind::Bark);
            let row = skeleton.chain_id.iter().position(|&id| id == instance);
            let order = skeleton.chain_order[row.expect("chain exists")];
            assert_eq!(order == 0, class == SemanticClass::Trunk);
        }
        SemanticClass::Twig | SemanticClass::Leaf => {
            assert_eq!(kind, PartKind::Leaf);
            assert!((instance as usize) < graph.organs().len());
        }
    }
}

/// The leaf part of the mesh of `plant`.
fn leaf_part(plant: &Plant) -> &arbrume_core::mesh::MeshPart {
    let found = plant.mesh.parts.iter().find(|x| x.kind == PartKind::Leaf);
    found.expect("a leaf part")
}

/// Number of vertices of each class in the leaf part.
fn leaf_part_classes(plant: &Plant, labels: &MeshLabels) -> [usize; 4] {
    let index = plant
        .mesh
        .parts
        .iter()
        .position(|part| part.kind == PartKind::Leaf)
        .unwrap();
    let mut counts = [0; 4];
    for &class in &labels.part(index).semantic {
        counts[class as usize] += 1;
    }
    counts
}

#[test]
fn twig_stems_are_labelled_twig() {
    let params = GenParams::new(4, 10).with_quality(0.5);
    for name in ["broadleaf", "conifer", "excurrent_conifer"] {
        let species = preset(name).unwrap();
        let (plant, labels) = labelled(&species, &params);
        let mut stemless = species.clone();
        stemless.foliage.twig.stem_radius = 0.0;
        let (bare, bare_labels) = labelled(&stemless, &params);
        let with = leaf_part_classes(&plant, &labels);
        let without = leaf_part_classes(&bare, &bare_labels);
        assert!(with[SemanticClass::Twig as usize] > 0, "{name}");
        assert_eq!(without[SemanticClass::Twig as usize], 0, "{name}");
        let stem_vertices = with[SemanticClass::Twig as usize];
        let vertices = |p: &Plant| leaf_part(p).buffers.positions.len();
        assert_eq!(vertices(&plant) - vertices(&bare), stem_vertices, "{name}");
        assert_eq!(
            with[SemanticClass::Leaf as usize],
            vertices(&bare),
            "{name}"
        );
        let part = leaf_part(&plant);
        let index = plant
            .mesh
            .parts
            .iter()
            .position(|x| x.kind == PartKind::Leaf);
        let own = labels.part(index.unwrap());
        let stem_uv = species
            .foliage
            .atlas()
            .map(0, arbrume_core::species::LeafAtlas::STEM_UV);
        for (vertex, &class) in own.semantic.iter().enumerate() {
            if class == SemanticClass::Twig as u8 {
                assert_eq!(
                    part.buffers.uvs[vertex], stem_uv,
                    "{name}: stems sample the stalk"
                );
            }
        }
    }
    let species = preset("sapling").unwrap();
    let (plant, labels) = labelled(&species, &params);
    assert_eq!(
        leaf_part_classes(&plant, &labels)[SemanticClass::Twig as usize],
        0
    );
}

/// Closest point to `p` on the triangle `a b c` (Ericson 2004, section 5.1.5).
fn closest_on_triangle(p: DVec3, a: DVec3, b: DVec3, c: DVec3) -> DVec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denominator = 1.0 / (va + vb + vc);
    a + ab * (vb * denominator) + ac * (vc * denominator)
}

/// Every triangle of `mesh` as corners, with its area.
fn triangles(mesh: &TreeMesh) -> Vec<([DVec3; 3], f64)> {
    let mut all = Vec::new();
    for part in &mesh.parts {
        let at = |i: u32| Vec3::from_array(part.buffers.positions[i as usize]).as_dvec3();
        for t in part.buffers.indices.as_chunks::<3>().0 {
            let corners = [at(t[0]), at(t[1]), at(t[2])];
            let area = (corners[1] - corners[0])
                .cross(corners[2] - corners[0])
                .length()
                * 0.5;
            all.push((corners, area));
        }
    }
    all
}

/// Index of the nearest triangle of each point and the largest distance.
fn nearest(cloud: &PointCloud, triangles: &[([DVec3; 3], f64)]) -> (Vec<usize>, f64) {
    let mut worst = 0.0_f64;
    let mut nearest = Vec::with_capacity(cloud.len());
    for point in &cloud.position {
        let p = Vec3::from_array(*point).as_dvec3();
        let (best, distance) = triangles
            .iter()
            .enumerate()
            .map(|(i, ([a, b, c], _))| (i, closest_on_triangle(p, *a, *b, *c).distance(p)))
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .unwrap();
        worst = worst.max(distance);
        nearest.push(best);
    }
    (nearest, worst)
}

/// Upper 0.999 quantile of the chi-square distribution with 15 degrees of freedom.
const CHI2_15_0999: f64 = 37.697;

/// Samples lie on the mesh, and their counts over 16 bins of equal area (consecutive
/// triangles) pass a chi-square test of uniformity at the 0.001 level. The sampler is
/// deterministic, so the test cannot be flaky: it either always passes or never does.
#[test]
fn points_lie_on_the_mesh_and_are_uniform_by_area() {
    let species = preset("thicket_shrub").unwrap();
    let (plant, labels) = labelled(&species, &GenParams::new(6, 4).with_quality(0.3));
    let all = triangles(&plant.mesh);
    assert!(
        all.len() < 20_000,
        "{} triangles: too slow for this test",
        all.len()
    );
    let count = 3000;
    let cloud = sample_points(&plant.mesh, &labels.parts(), count, 11);
    assert_eq!(cloud.len(), count);
    let size = f64::from(plant.stats.height.max(1e-3));
    let (nearest, worst) = nearest(&cloud, &all);
    assert!(worst <= size * 1e-5, "a point is {worst} m off the mesh");
    for normal in &cloud.normal {
        assert!((Vec3::from_array(*normal).length() - 1.0).abs() < 1e-4);
    }
    let total: f64 = all.iter().map(|(_, area)| area).sum();
    let bins = 16;
    let mut bin_of = Vec::with_capacity(all.len());
    let mut bin_area = vec![0.0_f64; bins];
    let mut running = 0.0;
    for (_, area) in &all {
        let bin = (((running + area * 0.5) / total * bins as f64) as usize).min(bins - 1);
        bin_of.push(bin);
        bin_area[bin] += area;
        running += area;
    }
    let mut observed = vec![0.0_f64; bins];
    for &triangle in &nearest {
        observed[bin_of[triangle]] += 1.0;
    }
    let chi2: f64 = (0..bins)
        .map(|b| {
            let expected = count as f64 * bin_area[b] / total;
            (observed[b] - expected).powi(2) / expected
        })
        .sum();
    assert!(chi2 < CHI2_15_0999, "chi-square {chi2:.2} over 16 bins");
}

/// Sidecar and point cloud bytes of plant `index`, generated on a pool of `threads`.
fn bytes_on(threads: usize, index: u64) -> (Vec<u8>, Vec<u8>) {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();
    pool.install(|| {
        let species = preset("excurrent_conifer").unwrap();
        let mut params = GenParams::new(index, 12).with_quality(0.5);
        params.mesh.parallel = true;
        let (plant, labels) = labelled(&species, &params);
        let mut sidecar = Vec::new();
        annotations(&plant, &labels)
            .write_npz(&mut sidecar)
            .unwrap();
        let mut ply = Vec::new();
        let cloud = sample_points(&plant.mesh, &labels.parts(), 2000, index);
        cloud.write_ply(&mut ply).unwrap();
        (sidecar, ply)
    })
}

#[test]
fn annotations_do_not_depend_on_the_thread_count() {
    for index in [1, 2] {
        assert_eq!(bytes_on(1, index), bytes_on(4, index));
    }
}
