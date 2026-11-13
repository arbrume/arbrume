//! Round-trip tests of the glb writer, parsed back with the `gltf` crate.
//!
//! When `ARBRUME_GLB_OUT` names a directory, every file written here is also saved there so
//! `tools/validate_glb.mjs` can run the Khronos validator on it.

mod common;

use arbrume_core::mesh::{MeshBuffers, MeshPart, PartKind, TreeMesh};
use arbrume_export::{GlbError, GlbOptions, write_glb};
use gltf::accessor::DataType;
use gltf::buffer::Source as BufferSource;
use gltf::image::Source as ImageSource;
use gltf::material::AlphaMode;

/// Writes `mesh`, checks the container framing and returns the bytes.
fn encode(name: &str, mesh: &TreeMesh) -> Vec<u8> {
    let options = GlbOptions {
        name: name.to_owned(),
        extras: Some(
            serde_json::json!({ "species": "test", "seed": 7, "parameters": { "height": 2.5 } }),
        ),
        ..GlbOptions::default()
    };
    let mut bytes = Vec::new();
    write_glb(mesh, &options, &mut bytes).expect("write glb");
    check_framing(&bytes);
    if let Some(dir) = std::env::var_os("ARBRUME_GLB_OUT") {
        std::fs::create_dir_all(&dir).expect("create output dir");
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}.glb")),
            &bytes,
        )
        .expect("save glb");
    }
    bytes
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// Checks header, chunk types, lengths and 4-byte alignment of the glb container.
fn check_framing(bytes: &[u8]) {
    assert_eq!(word(bytes, 0), 0x4654_6C67, "magic");
    assert_eq!(word(bytes, 4), 2, "version");
    assert_eq!(word(bytes, 8) as usize, bytes.len(), "total length");
    let json_len = word(bytes, 12) as usize;
    assert_eq!(word(bytes, 16), 0x4E4F_534A, "JSON chunk type");
    assert_eq!(json_len % 4, 0, "JSON chunk padded");
    let bin_at = 20 + json_len;
    if bin_at < bytes.len() {
        assert_eq!(word(bytes, bin_at + 4), 0x004E_4942, "BIN chunk type");
        let bin_len = word(bytes, bin_at) as usize;
        assert_eq!(bin_len % 4, 0, "BIN chunk padded");
        assert_eq!(
            bin_at + 8 + bin_len,
            bytes.len(),
            "BIN chunk fills the file"
        );
    } else {
        assert_eq!(bin_at, bytes.len());
    }
}

fn parse(bytes: &[u8]) -> gltf::Gltf {
    let gltf = gltf::Gltf::from_slice(bytes).expect("valid glTF");
    for view in gltf.views() {
        assert_eq!(view.offset() % 4, 0, "buffer view {} aligned", view.index());
    }
    gltf
}

fn extras_json(raw: &gltf::json::Extras) -> serde_json::Value {
    serde_json::from_str(raw.as_ref().expect("extras").get()).unwrap()
}

#[test]
fn round_trip_two_parts() {
    let mesh = common::sample_mesh();
    let bytes = encode("two_parts", &mesh);
    let gltf = parse(&bytes);
    let blob = gltf.blob.as_deref();
    let json = gltf.document.as_json();
    assert!(
        json.asset
            .generator
            .as_deref()
            .unwrap()
            .starts_with("Arbrume ")
    );
    assert_eq!(extras_json(&json.asset.extras)["seed"], 7);
    let node = gltf.nodes().next().unwrap();
    assert_eq!(node.name(), Some("two_parts"));
    assert_eq!(extras_json(node.extras())["species"], "test");
    assert_eq!(gltf.buffers().count(), 1);
    assert_eq!(gltf.materials().count(), 2);

    let gmesh = node.mesh().expect("node has the mesh");
    let primitives: Vec<_> = gmesh.primitives().collect();
    assert_eq!(primitives.len(), mesh.parts.len());
    for (primitive, part) in primitives.iter().zip(&mesh.parts) {
        let reader = primitive.reader(|b| {
            matches!(b.source(), BufferSource::Bin)
                .then_some(blob)
                .flatten()
        });
        let positions: Vec<[f32; 3]> = reader.read_positions().unwrap().collect();
        assert_eq!(positions, part.buffers.positions);
        let indices: Vec<u32> = reader.read_indices().unwrap().into_u32().collect();
        assert_eq!(indices, part.buffers.indices);
        assert_eq!(primitive.indices().unwrap().data_type(), DataType::U16);
        let normals: Vec<[f32; 3]> = reader.read_normals().unwrap().collect();
        assert_eq!(normals, part.buffers.normals);
        let label = extras_json(primitive.extras())["semantic"].clone();
        assert_eq!(
            label,
            if part.kind == PartKind::Bark {
                "bark"
            } else {
                "leaf"
            }
        );
        check_bounds(primitive, &part.buffers.positions);
    }
    let colors: Vec<[f32; 4]> = primitives[0]
        .reader(|_| blob)
        .read_colors(0)
        .unwrap()
        .into_rgba_f32()
        .collect();
    assert_eq!(colors, mesh.parts[0].buffers.colors);
    assert!(primitives[0].get(&gltf::Semantic::TexCoords(0)).is_none());
    assert!(primitives[1].get(&gltf::Semantic::Colors(0)).is_none());
    check_materials(&gltf, &mesh);
}

fn check_bounds(primitive: &gltf::Primitive, positions: &[[f32; 3]]) {
    let accessor = primitive.get(&gltf::Semantic::Positions).unwrap();
    let to_vec = |v: gltf::json::Value| -> Vec<f32> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_f64().unwrap() as f32)
            .collect()
    };
    let min = to_vec(accessor.min().expect("POSITION min"));
    let max = to_vec(accessor.max().expect("POSITION max"));
    for axis in 0..3 {
        let values = positions.iter().map(|p| p[axis]);
        assert_eq!(min[axis], values.clone().fold(f32::INFINITY, f32::min));
        assert_eq!(max[axis], values.fold(f32::NEG_INFINITY, f32::max));
    }
}

fn check_materials(gltf: &gltf::Gltf, mesh: &TreeMesh) {
    for (material, part) in gltf.materials().zip(&mesh.parts) {
        let expected = &part.material;
        let pbr = material.pbr_metallic_roughness();
        assert_eq!(material.name(), Some(expected.name.as_str()));
        assert_eq!(pbr.base_color_factor(), expected.base_color);
        assert_eq!(pbr.roughness_factor(), expected.roughness);
        assert_eq!(pbr.metallic_factor(), 0.0);
        assert_eq!(material.double_sided(), expected.double_sided);
        assert_eq!(material.alpha_cutoff(), expected.alpha_cutoff);
        let mode = if expected.alpha_cutoff.is_some() {
            AlphaMode::Mask
        } else {
            AlphaMode::Opaque
        };
        assert_eq!(material.alpha_mode(), mode);
        assert_eq!(
            pbr.base_color_texture().is_some(),
            expected.base_color_texture.is_some()
        );
    }
}

#[test]
fn embedded_texture_round_trips() {
    let bytes = encode("texture", &common::sample_mesh());
    let gltf = parse(&bytes);
    let blob = gltf.blob.as_deref().unwrap();
    let image = gltf.images().next().expect("one image");
    let ImageSource::View { view, mime_type } = image.source() else {
        panic!("image must be stored in a buffer view");
    };
    assert_eq!(mime_type, "image/png");
    assert!(view.target().is_none());
    assert_eq!(
        &blob[view.offset()..view.offset() + view.length()],
        common::TINY_PNG.as_slice()
    );
}

#[test]
fn jpeg_is_detected_and_unknown_image_rejected() {
    let mut mesh = common::sample_mesh();
    let jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0];
    mesh.parts[1].material.base_color_texture = Some(jpeg);
    let mut bytes = Vec::new();
    write_glb(&mesh, &GlbOptions::default(), &mut bytes).unwrap();
    let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
    let source = gltf.images().next().unwrap().source();
    assert!(matches!(
        source,
        ImageSource::View {
            mime_type: "image/jpeg",
            ..
        }
    ));

    mesh.parts[1].material.base_color_texture = Some(b"GIF89a".to_vec());
    let err = write_glb(&mesh, &GlbOptions::default(), Vec::new()).unwrap_err();
    assert!(matches!(err, GlbError::ImageFormat { part: 1 }));
}

#[test]
fn empty_mesh_is_a_valid_empty_scene() {
    let empty_part = MeshPart {
        buffers: MeshBuffers::default(),
        ..common::sample_mesh().parts.remove(0)
    };
    for (name, mesh) in [
        ("empty", TreeMesh::default()),
        (
            "empty_part",
            TreeMesh {
                parts: vec![empty_part],
            },
        ),
    ] {
        let bytes = encode(name, &mesh);
        let gltf = parse(&bytes);
        assert!(gltf.blob.is_none());
        assert_eq!(gltf.meshes().count(), 0);
        assert_eq!(gltf.buffers().count(), 0);
        assert_eq!(gltf.nodes().count(), 1);
    }
}

#[test]
fn part_without_optional_attributes() {
    let mut part = common::sample_mesh().parts.remove(1);
    part.buffers.normals.clear();
    part.buffers.uvs.clear();
    let bytes = encode("positions_only", &TreeMesh { parts: vec![part] });
    let gltf = parse(&bytes);
    let primitive = gltf.meshes().next().unwrap().primitives().next().unwrap();
    assert_eq!(primitive.attributes().count(), 1);
    assert!(
        gltf.images().next().is_none(),
        "texture without uvs is not embedded"
    );
    assert!(
        primitive
            .material()
            .pbr_metallic_roughness()
            .base_color_texture()
            .is_none()
    );
}

/// A strip of `vertices` points (at least 3) with one triangle fan touching the last one.
fn strip(vertices: usize) -> TreeMesh {
    let positions: Vec<[f32; 3]> = (0..vertices)
        .map(|i| [i as f32 * 1e-3, (i % 2) as f32, 0.0])
        .collect();
    let last = (vertices - 1) as u32;
    let indices = (0..last - 1).flat_map(|i| [i, i + 1, last]).collect();
    let mut part = common::sample_mesh().parts.remove(0);
    part.buffers = MeshBuffers {
        positions,
        indices,
        ..MeshBuffers::default()
    };
    TreeMesh { parts: vec![part] }
}

#[test]
fn index_width_switches_at_65536_vertices() {
    for (vertices, width) in [(65_535, DataType::U16), (65_536, DataType::U32)] {
        let mesh = strip(vertices);
        let bytes = encode(&format!("strip_{vertices}"), &mesh);
        let gltf = parse(&bytes);
        let blob = gltf.blob.as_deref();
        let primitive = gltf.meshes().next().unwrap().primitives().next().unwrap();
        assert_eq!(primitive.indices().unwrap().data_type(), width);
        let indices: Vec<u32> = primitive
            .reader(|_| blob)
            .read_indices()
            .unwrap()
            .into_u32()
            .collect();
        assert_eq!(indices, mesh.parts[0].buffers.indices);
    }
}

#[test]
fn malformed_parts_are_rejected() {
    let base = common::sample_mesh();
    let mut mesh = base.clone();
    mesh.parts[1].buffers.uvs.pop();
    let err = write_glb(&mesh, &GlbOptions::default(), Vec::new()).unwrap_err();
    assert!(matches!(
        err,
        GlbError::AttributeLength {
            part: 1,
            attribute: "uvs",
            ..
        }
    ));

    let mut mesh = base.clone();
    mesh.parts[0].buffers.indices.pop();
    let err = write_glb(&mesh, &GlbOptions::default(), Vec::new()).unwrap_err();
    assert!(matches!(err, GlbError::IndexCount { part: 0, .. }));

    let mut mesh = base.clone();
    mesh.parts[1].buffers.indices[0] = 4;
    let err = write_glb(&mesh, &GlbOptions::default(), Vec::new()).unwrap_err();
    assert!(matches!(
        err,
        GlbError::IndexRange {
            part: 1,
            index: 4,
            ..
        }
    ));

    let mut mesh = base;
    mesh.parts[0].buffers.positions[3][1] = f32::NAN;
    let err = write_glb(&mesh, &GlbOptions::default(), Vec::new()).unwrap_err();
    assert!(matches!(
        err,
        GlbError::NonFinitePosition { part: 0, vertex: 3 }
    ));
}
