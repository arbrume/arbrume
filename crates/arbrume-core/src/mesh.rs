//! Mesh buffers shared by the mesher and the exporters.
//!
//! A [`TreeMesh`] is a list of parts, one per material. Buffers are flat `f32`/`u32`
//! vectors so they can be handed to glTF, `NumPy` or Blender without conversion.
//! Coordinates are meters, right-handed, Y up (the glTF convention).

/// Semantic class of a part, written to exports as the ML label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartKind {
    /// Trunk and branches.
    Bark,
    /// Leaves, needles and other foliage.
    Leaf,
    /// Textured planes standing for the whole plant at a distance (billboards or
    /// impostors), with an alpha-masked atlas.
    Billboard,
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
    /// Encoded PNG bytes of the tangent-space normal texture (glTF convention: `+Y`
    /// towards increasing `v` of `TEXCOORD_0` as stored). Its alpha channel may carry
    /// other data, which glTF readers ignore.
    pub normal_texture: Option<Vec<u8>>,
}

/// Indexed triangle buffers. `normals`, `uvs`, `colors` and `wind` are either empty or
/// have one entry per position.
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
    /// Baked wind data per vertex, see [`WindVertex`]: `[x0, x1, x2, x3, branch, flutter]`.
    pub wind: Vec<WindVertex>,
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

/// Baked wind data of one vertex: `[x0, x1, x2, x3, branch, flutter]`.
///
/// `x0..x3` are the normalized positions, in `[0, 1]` from base to tip, along the
/// branches of hierarchy levels 0 to 3 that carry the vertex (Habel et al. 2009): `x_k`
/// is where the lineage of the vertex leaves the branch of level `k`, or where the vertex
/// itself lies on it for its own level, and 0 below its own level. `branch` is the index
/// of the own branch of the vertex in [`WindTable::branches`], stored as a float (exact
/// below 2^24). `flutter` is 0 on bark and grows from 0 at the attachment of a leaf or
/// twig to 1 at its farthest point.
pub type WindVertex = [f32; 6];

/// Per-branch wind data of one mesh; one entry per meshed axis.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WindTable {
    /// The branches, parents before children.
    pub branches: Vec<WindBranch>,
}

/// Wind data of one branch (one axis of the plant).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindBranch {
    /// Chain id of the axis in the plant graph.
    pub chain: u32,
    /// Index of the parent branch, `None` for a root.
    pub parent: Option<u32>,
    /// Hierarchy level in `0..=3`; axes deeper than level 3 are given level 3 and take
    /// their pivot from their own base.
    pub level: u8,
    /// The branch of each level `0..=level` along its lineage, itself at `level`.
    pub lineage: [u32; 4],
    /// `x` of the lineage on the branches of the levels below `level`, the values
    /// `x0..x3` every vertex of the branch carries below its own level; 0 from `level` on.
    pub lineage_x: [f32; 4],
    /// Base of the branch, the pivot it bends about.
    pub pivot: [f32; 3],
    /// Unit direction of the branch at its base (`t` of Habel et al.).
    pub tangent: [f32; 3],
    /// Unit vector orthogonal to `tangent`, as horizontal as possible (`r`); the third
    /// axis of the frame is `tangent x right` (`s`).
    pub right: [f32; 3],
    /// Length along the axis, in meters.
    pub length: f32,
    /// Radius at the base, in meters.
    pub root_radius: f32,
    /// Radius at the tip, in meters.
    pub tip_radius: f32,
    /// `tip_radius / root_radius`, the taper ratio `alpha` of Habel et al.
    pub taper: f32,
    /// Wood density times the volume of the branch and of every branch it bears.
    pub mass: f32,
    /// Coefficients of the deflection shape `u(x) = c2 x^2 + c4 x^4` of a tapered
    /// cantilever under a uniform load, normalized so that `u(1) = 1`.
    pub shape: [f32; 2],
}

/// The complete mesh of one plant.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TreeMesh {
    /// Parts in draw order.
    pub parts: Vec<MeshPart>,
    /// Per-branch wind data, when the mesh was built with it; the parts then carry
    /// [`MeshBuffers::wind`].
    pub wind: Option<WindTable>,
}

impl TreeMesh {
    /// Total number of triangles over all parts.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.parts.iter().map(|p| p.buffers.triangle_count()).sum()
    }
}

/// Semantic class of a mesh vertex, the per-vertex ML label.
///
/// Finer than [`PartKind`]: the bark part holds the trunk (axes of order 0) and the
/// branches (every other axis), and the leaf part holds the leaves and, for a species
/// with twig templates, the twig stems. The numeric values are those written to files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum SemanticClass {
    /// An axis of branching order 0 (the trunk, or each stem of a multi-stemmed plant).
    Trunk = 0,
    /// An axis of branching order 1 or more.
    Branch = 1,
    /// The stem of a twig template, instanced at an organ site with its leaves.
    Twig = 2,
    /// A leaf, a needle, or the fins that stand for needles on a twig.
    Leaf = 3,
}

impl SemanticClass {
    /// Every class, in the order of their numeric values.
    pub const ALL: [Self; 4] = [Self::Trunk, Self::Branch, Self::Twig, Self::Leaf];

    /// Lower-case name written to files: `trunk`, `branch`, `twig` or `leaf`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Trunk => "trunk",
            Self::Branch => "branch",
            Self::Twig => "twig",
            Self::Leaf => "leaf",
        }
    }

    /// The class with numeric value `value`, `None` past the last.
    #[must_use]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Trunk),
            1 => Some(Self::Branch),
            2 => Some(Self::Twig),
            3 => Some(Self::Leaf),
            _ => None,
        }
    }
}

/// A run of consecutive vertices of one part that come from the same source.
///
/// `instance` is the chain id of the axis for [`SemanticClass::Trunk`] and
/// [`SemanticClass::Branch`], and the organ index of the site for
/// [`SemanticClass::Twig`] and [`SemanticClass::Leaf`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VertexRun {
    /// Index of the first vertex in the buffers of the part.
    pub start: u32,
    /// Number of vertices.
    pub len: u32,
    /// Class of every vertex of the run.
    pub class: SemanticClass,
    /// Chain id or organ index.
    pub instance: u32,
}

/// Where the vertices of a [`TreeMesh`] come from: for each part, in the order of
/// `TreeMesh::parts`, runs that cover its vertices in order. Every triangle lies within
/// one run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MeshProvenance {
    /// One entry per part: its kind and its runs.
    pub parts: Vec<(PartKind, Vec<VertexRun>)>,
}

/// Per-vertex labels of one part: the runs of its [`MeshProvenance`] expanded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartLabels {
    /// [`SemanticClass`] value of each vertex.
    pub semantic: Vec<u8>,
    /// Instance id of each vertex (chain id or organ index, see [`VertexRun`]).
    pub instance: Vec<u32>,
}
