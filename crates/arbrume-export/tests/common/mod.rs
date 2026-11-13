//! Small two-part tree shared by the integration tests and the `sample_glb` example.

use arbrume_core::mesh::{Material, MeshBuffers, MeshPart, PartKind, TreeMesh};

/// A 2x2 RGBA PNG (two green texels, two transparent ones).
pub const TINY_PNG: [u8; 77] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x06, 0x00, 0x00, 0x00, 0x72, 0xb6, 0x0d,
    0x24, 0x00, 0x00, 0x00, 0x14, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xd0, 0xc8, 0x93, 0xfb,
    0x6f, 0xd3, 0xa3, 0xc1, 0xc0, 0x00, 0x63, 0x00, 0x00, 0x31, 0x77, 0x05, 0x47, 0xfa, 0xa8, 0x55,
    0xa1, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// A square trunk prism (bark, opaque, vertex colours) and a textured leaf card.
pub fn sample_mesh() -> TreeMesh {
    TreeMesh {
        parts: vec![trunk(), leaf_card()],
    }
}

fn trunk() -> MeshPart {
    let corners: [[f32; 2]; 4] = [[0.1, 0.1], [-0.1, 0.1], [-0.1, -0.1], [0.1, -0.1]];
    let mut buffers = MeshBuffers::default();
    for (i, [x, z]) in corners.into_iter().enumerate() {
        let next = corners[(i + 1) % 4];
        let (nx, nz) = (x + next[0], z + next[1]);
        let len = (nx * nx + nz * nz).sqrt();
        let normal = [nx / len, 0.0, nz / len];
        for (px, pz) in [(x, z), (next[0], next[1])] {
            for y in [0.0, 2.0] {
                buffers.positions.push([px, y, pz]);
                buffers.normals.push(normal);
                buffers.colors.push([0.35, 0.25, 0.15, 1.0]);
            }
        }
        let base = (i * 4) as u32;
        buffers
            .indices
            .extend([base, base + 1, base + 2, base + 1, base + 3, base + 2]);
    }
    MeshPart {
        kind: PartKind::Bark,
        material: Material {
            name: "bark".to_owned(),
            base_color: [1.0, 1.0, 1.0, 1.0],
            roughness: 0.9,
            double_sided: false,
            alpha_cutoff: None,
            base_color_texture: None,
        },
        buffers,
    }
}

fn leaf_card() -> MeshPart {
    MeshPart {
        kind: PartKind::Leaf,
        material: Material {
            name: "leaf".to_owned(),
            base_color: [0.4, 0.8, 0.3, 1.0],
            roughness: 0.6,
            double_sided: true,
            alpha_cutoff: Some(0.5),
            base_color_texture: Some(TINY_PNG.to_vec()),
        },
        buffers: MeshBuffers {
            positions: vec![
                [-0.3, 2.0, 0.0],
                [0.3, 2.0, 0.0],
                [0.3, 2.6, 0.0],
                [-0.3, 2.6, 0.0],
            ],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            uvs: vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
            colors: Vec::new(),
            indices: vec![0, 1, 2, 0, 2, 3],
        },
    }
}
