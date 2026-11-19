//! Annotation sidecars of hand-built graphs, and custom vertex attributes in `.glb`.

#[path = "common/mod.rs"]
mod common;

use arbrume_core::graph::{ChainKind, Metamer, OrganKind, OrganSite, PlantGraph};
use arbrume_export::annotations::{Annotations, BranchTable, GlbLabels, MeshLabels, Skeleton};
use arbrume_export::{AttributeData, ExtraAttribute, GlbError, GlbOptions, write_glb_with};
use glam::Vec3;

fn at(x: f32, y: f32, birth: u16) -> Metamer {
    Metamer::new(Vec3::new(x, y, 0.0), 0.1 / (1.0 + y), birth)
}

/// Two root axes, a lateral grown after its parent was extended (so the arena is not
/// in depth-first order), a relay axis, a shed lateral and two organ sites.
fn graph() -> PlantGraph {
    let mut graph = PlantGraph::new();
    let base = graph.add_root_chain(Vec3::ZERO, at(0.0, 1.0, 0));
    let trunk = graph.chain_of(base);
    let middle = graph.extend_chain(trunk, at(0.0, 2.0, 1));
    let side = graph.add_chain(base, ChainKind::Lateral, at(1.0, 1.5, 1));
    let top = graph.extend_chain(trunk, at(0.0, 3.0, 2));
    graph.extend_chain(graph.chain_of(side), at(2.0, 2.0, 2));
    graph.add_chain(top, ChainKind::Relay, at(0.5, 4.0, 3));
    let shed = graph.add_chain(middle, ChainKind::Lateral, at(-1.0, 2.5, 2));
    graph.extend_chain(graph.chain_of(shed), at(-2.0, 3.0, 3));
    graph.shed_subtree(shed);
    let other = graph.add_root_chain(Vec3::X * 5.0, at(5.0, 1.0, 0));
    for node in [top, other] {
        graph.add_organ(OrganSite {
            node,
            kind: OrganKind::Leaf,
            direction: Vec3::X,
            normal: Vec3::Y,
            scale: 1.0,
            birth: 2,
        });
    }
    graph.check_invariants().unwrap();
    assert!(!graph.is_contiguous());
    graph
}

#[test]
fn a_skeleton_rebuilds_its_graph() {
    let graph = graph();
    let skeleton = Skeleton::from_graph(&graph);
    assert_eq!(skeleton.node_parent, [-1, 0, 1, 2, 1, 4, 0, 6, -1]);
    assert_eq!(skeleton.chain_id, [0, 2, 3, 1, 4]);
    assert_eq!(skeleton.node_shed, [0, 0, 0, 0, 1, 1, 0, 0, 0]);
    assert_eq!(skeleton.chain_parent, [-1, 0, 0, 0, -1]);
    let dist = skeleton.node_dist_root[3];
    assert!((dist - (3.0 + 0.5_f32.hypot(1.0))).abs() < 1e-5, "{dist}");
    let rebuilt = skeleton.to_graph().unwrap();
    rebuilt.check_invariants().unwrap();
    let mut compact = graph.clone();
    compact.compact();
    assert_eq!(rebuilt.positions(), compact.positions());
    assert_eq!(rebuilt.parents(), compact.parents());
    assert_eq!(rebuilt.node_chains(), compact.node_chains());
    assert_eq!(rebuilt.organs(), compact.organs());
    assert_eq!(rebuilt.chains().len(), graph.chains().len());
    assert_eq!(Skeleton::from_graph(&rebuilt), skeleton);
}

#[test]
fn the_branch_table_leaves_out_shed_axes() {
    let table = BranchTable::from_graph(&graph(), 3);
    assert_eq!(table.chain, [0, 2, 1, 4]);
    assert_eq!(table.parent, [-1, 0, 0, -1]);
    assert_eq!(table.order, [0, 0, 1, 0]);
    assert_eq!(table.nodes, [3, 1, 2, 1]);
    assert!((table.attach[1] - 1.0).abs() < 1e-6);
    assert!((table.attach[2] - 1.0 / 3.0).abs() < 1e-6);
    assert_eq!(table.start[0], [0.0, 0.0, 0.0, 0.1 / 2.0]);
    assert_eq!(table.polyline[1], [0.0, 1.5, 0.0, 0.1 / 3.0]);
    assert_eq!(table.end[0], [0.0, 3.0, 0.0, 0.1 / 4.0]);
}

#[test]
fn inconsistent_skeletons_and_archives_are_rejected() {
    let mut skeleton = Skeleton::from_graph(&graph());
    skeleton.chain_id[1] = 0;
    assert!(skeleton.to_graph().is_err());
    let annotations = Annotations {
        skeleton: Some(Skeleton::from_graph(&graph())),
        ..Annotations::default()
    };
    let mut bytes = Vec::new();
    annotations.write_npz(&mut bytes).unwrap();
    assert_eq!(Annotations::read_npz(&bytes).unwrap(), annotations);
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0xFF;
    assert!(Annotations::read_npz(&bytes).is_err());
    assert!(Annotations::read_npz(&bytes[..100]).is_err());
}

#[test]
fn json_has_the_arrays_of_the_archive() {
    let annotations = Annotations {
        skeleton: Some(Skeleton::from_graph(&graph())),
        branches: Some(BranchTable::from_graph(&graph(), 2)),
        ..Annotations::default()
    };
    let json = annotations.to_json();
    assert_eq!(json["meta"]["version"], 1);
    assert_eq!(json["node_parent"][0], -1);
    assert_eq!(json["node_position"][2][1], 3.0);
    assert_eq!(json["branch_polyline"][0][1][3], f64::from(0.1_f32 / 4.0));
    let described = json["meta"]["arrays"].as_object().unwrap();
    assert_eq!(described.len(), json.as_object().unwrap().len() - 1);
}

#[test]
fn label_attributes_reach_the_glb() {
    let mesh = common::sample_mesh();
    let counts: Vec<u32> = mesh
        .parts
        .iter()
        .map(|part| part.buffers.positions.len() as u32)
        .collect();
    let labels = MeshLabels {
        part_kind: vec![0, 1],
        part_offsets: vec![0, counts[0], counts[0] + counts[1]],
        semantic: (0..counts[0] + counts[1]).map(|v| (v % 4) as u8).collect(),
        instance: (0..counts[0] + counts[1]).collect(),
    };
    let glb_labels = GlbLabels::new(&labels).unwrap();
    let mut bytes = Vec::new();
    let options = GlbOptions::default();
    write_glb_with(&mesh, &options, &glb_labels.attributes(), &mut bytes).unwrap();
    let document = gltf::Gltf::from_slice(&bytes).unwrap();
    let blob = document.blob.as_deref().unwrap();
    for (part, primitive) in document.meshes().next().unwrap().primitives().enumerate() {
        let accessor = primitive
            .get(&gltf::Semantic::Extras("INSTANCE".to_owned()))
            .unwrap();
        let view = accessor.view().unwrap();
        let data = &blob[view.offset()..view.offset() + view.length()];
        let values: Vec<f32> = data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        let first = labels.part_offsets[part];
        let expected: Vec<f32> = (first..first + counts[part]).map(|v| v as f32).collect();
        assert_eq!(values, expected);
    }
}

#[test]
fn bad_extra_attributes_are_rejected() {
    let mesh = common::sample_mesh();
    let values = vec![0.0_f32; 64];
    let attribute = |part: usize, name: &str, length: usize| ExtraAttribute {
        part,
        name: name.to_owned(),
        components: 1,
        data: AttributeData::F32(&values[..length]),
    };
    let vertices = mesh.parts[0].buffers.positions.len();
    let cases = [
        vec![attribute(0, "LABEL", vertices)],
        vec![attribute(5, "_LABEL", vertices)],
        vec![attribute(0, "_LABEL", vertices - 1)],
        vec![
            attribute(0, "_LABEL", vertices),
            attribute(0, "_LABEL", vertices),
        ],
    ];
    for extra in cases {
        let result = write_glb_with(&mesh, &GlbOptions::default(), &extra, Vec::new());
        assert!(matches!(result, Err(GlbError::ExtraAttribute { .. })));
    }
}
