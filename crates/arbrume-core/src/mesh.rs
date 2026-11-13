//! Mesh buffers shared by the mesher and the exporters.
//!
//! A [`TreeMesh`] is a list of parts, one per material. Buffers are flat `f32`/`u32`
//! vectors so they can be handed to glTF, NumPy or Blender without conversion.
//! Coordinates are meters, right-handed, Y up (the glTF convention).

/// Semantic class of a part, written to exports as the ML label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartKind {
    /// Trunk and branches.
    Bark,
    /// Leaves, needles and other foliage.
    Leaf,
}

/// Surface description of one part. Textures are optional and embedded.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    /// Material name written to the export.
    pub name: String,
    /// Linear RGBA base colour factor.
    pub base_color: [f32; 4],
    /// Roughness factor in `[0, 1]`.
    pub roughness: f32,
    /// Render both faces (leaf cards).
    pub double_sided: bool,
    /// Alpha cutoff for masked materials, `None` for opaque.
    pub alpha_cutoff: Option<f32>,
    /// Encoded PNG or JPEG bytes of the base colour texture.
    pub base_color_texture: Option<Vec<u8>>,
}

/// Indexed triangle buffers. `normals`, `uvs` and `colors` are either empty or have one
/// entry per position.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshBuffers {
    /// Vertex positions.
    pub positions: Vec<[f32; 3]>,
    /// Unit vertex normals.
    pub normals: Vec<[f32; 3]>,
    /// Texture coordinates.
    pub uvs: Vec<[f32; 2]>,
    /// Linear RGBA vertex colours, multiplied with the material base colour.
    pub colors: Vec<[f32; 4]>,
    /// Triangle list, counter-clockwise front faces.
    pub indices: Vec<u32>,
}

impl MeshBuffers {
    /// Number of triangles.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// One drawable part of a tree: buffers plus a material.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshPart {
    /// Semantic class.
    pub kind: PartKind,
    /// Material of the whole part.
    pub material: Material,
    /// Geometry.
    pub buffers: MeshBuffers,
}

/// The complete mesh of one plant.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TreeMesh {
    /// Parts in draw order.
    pub parts: Vec<MeshPart>,
}

impl TreeMesh {
    /// Total number of triangles over all parts.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.parts.iter().map(|p| p.buffers.triangle_count()).sum()
    }
}
