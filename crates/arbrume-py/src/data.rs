//! The Rust-side storage of one generated tree, shared by every Python object that
//! views it.
//!
//! A [`TreeData`] is built once, outside the GIL, and never changes afterwards: every
//! array handed to Python points into it, and every Python object holding such an array
//! keeps an `Arc` to it. Mesh buffers and the graph columns that already have a plain
//! numeric type (radii, orders, births) are exposed as they are; columns whose Rust type
//! is a newtype or a `glam` vector (parents, chains, positions, organ sites) are flattened
//! once into the [`SkeletonArrays`] here.

use std::sync::Arc;

use arbrume_core::graph::{OrganKind, PlantGraph};
use arbrume_core::species::Species;
use arbrume_gen::Plant;
use bytemuck::cast_slice;

/// Organ kinds in the order of their codes in `Organs.kinds`.
pub const ORGAN_KINDS: [OrganKind; 4] = [
    OrganKind::Leaf,
    OrganKind::Twig,
    OrganKind::Flower,
    OrganKind::Fruit,
];

/// Element type of an exported array, with its buffer-protocol format character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Element {
    /// `float32`.
    F32,
    /// `uint32`.
    U32,
    /// `int32`.
    I32,
    /// `uint16`.
    U16,
    /// `uint8`.
    U8,
    /// `bool`, one byte holding 0 or 1.
    Bool,
}

impl Element {
    /// Struct-module format string of the element.
    pub const fn format(self) -> &'static std::ffi::CStr {
        match self {
            Self::F32 => c"f",
            Self::U32 => c"I",
            Self::I32 => c"i",
            Self::U16 => c"H",
            Self::U8 => c"B",
            Self::Bool => c"?",
        }
    }

    /// Size of one element in bytes.
    pub const fn size(self) -> usize {
        match self {
            Self::F32 | Self::U32 | Self::I32 => 4,
            Self::U16 => 2,
            Self::U8 | Self::Bool => 1,
        }
    }
}

/// Bytes of one array, its element type and its number of columns (1 for a flat array).
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    /// The raw bytes, C-contiguous rows.
    pub bytes: &'a [u8],
    /// Element type.
    pub element: Element,
    /// Elements per row.
    pub columns: usize,
}

impl View<'_> {
    /// Number of rows.
    pub const fn rows(&self) -> usize {
        self.bytes.len() / (self.element.size() * self.columns)
    }
}

/// An array of one mesh part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartField {
    /// Vertex positions, `(n, 3)`.
    Positions,
    /// Vertex normals, `(n, 3)`.
    Normals,
    /// Texture coordinates, `(n, 2)`.
    Uvs,
    /// Vertex colours, `(n, 4)`.
    Colors,
    /// Triangles, `(m, 3)`.
    Indices,
}

/// An array of the skeleton or of the organ sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphField {
    /// Node positions, `(n, 3)`.
    Positions,
    /// Internode radii.
    Radii,
    /// Parent node, -1 for the first node of a root chain.
    Parents,
    /// Branching order.
    Orders,
    /// Growth cycle of appearance.
    Births,
    /// Chain of each node.
    Chains,
    /// Whether each node was shed.
    Shed,
    /// Node bearing each organ.
    OrganNodes,
    /// Kind code of each organ.
    OrganKinds,
    /// Unit main direction of each organ, `(k, 3)`.
    OrganDirections,
    /// Unit normal of each organ, `(k, 3)`.
    OrganNormals,
    /// Size factor of each organ.
    OrganScales,
    /// Growth cycle of appearance of each organ.
    OrganBirths,
}

/// Which array of a tree a buffer exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// An array of the mesh part with this index.
    Part(usize, PartField),
    /// An array of the skeleton or organ sites.
    Graph(GraphField),
}

/// Graph columns flattened to plain numeric types.
#[derive(Debug, Clone, Default)]
pub struct SkeletonArrays {
    positions: Vec<[f32; 3]>,
    parents: Vec<i32>,
    chains: Vec<i32>,
    shed: Vec<u8>,
    organ_nodes: Vec<i32>,
    organ_kinds: Vec<u8>,
    organ_directions: Vec<[f32; 3]>,
    organ_normals: Vec<[f32; 3]>,
    organ_scales: Vec<f32>,
    organ_births: Vec<u16>,
}

/// A `u32` id as `i32`, the absent id `u32::MAX` becoming -1.
fn signed(id: u32) -> i32 {
    i32::try_from(id).unwrap_or(-1)
}

/// Code of an organ kind: its index in [`ORGAN_KINDS`].
fn organ_code(kind: OrganKind) -> u8 {
    ORGAN_KINDS
        .iter()
        .position(|known| *known == kind)
        .map_or(u8::MAX, |index| index as u8)
}

impl SkeletonArrays {
    /// Flattens the columns of `graph`.
    fn of(graph: &PlantGraph) -> Self {
        let organs = graph.organs();
        Self {
            positions: graph.positions().iter().copied().map(Into::into).collect(),
            parents: graph.parents().iter().map(|id| signed(id.0)).collect(),
            chains: graph.node_chains().iter().map(|id| signed(id.0)).collect(),
            shed: graph
                .flags()
                .iter()
                .map(|f| u8::from(f.is_shed()))
                .collect(),
            organ_nodes: organs.iter().map(|site| signed(site.node.0)).collect(),
            organ_kinds: organs.iter().map(|site| organ_code(site.kind)).collect(),
            organ_directions: organs
                .iter()
                .map(|site| site.direction.to_array())
                .collect(),
            organ_normals: organs.iter().map(|site| site.normal.to_array()).collect(),
            organ_scales: organs.iter().map(|site| site.scale).collect(),
            organ_births: organs.iter().map(|site| site.birth).collect(),
        }
    }
}

/// One generated tree: its species, the plant and the flattened skeleton.
#[derive(Debug, Clone)]
pub struct TreeData {
    /// Species the tree was generated from.
    pub species: Arc<Species>,
    /// The plant: parameters, graph, mesh and statistics.
    pub plant: Plant,
    skeleton: SkeletonArrays,
}

impl TreeData {
    /// Wraps a generated plant.
    pub fn new(species: Arc<Species>, plant: Plant) -> Self {
        let skeleton = SkeletonArrays::of(&plant.graph);
        Self {
            species,
            plant,
            skeleton,
        }
    }

    /// The bytes and layout of `field`. A part index past the last part gives an empty
    /// array.
    pub fn view(&self, field: Field) -> View<'_> {
        match field {
            Field::Part(index, part) => self.part_view(index, part),
            Field::Graph(graph) => self.graph_view(graph),
        }
    }

    fn part_view(&self, index: usize, field: PartField) -> View<'_> {
        let Some(part) = self.plant.mesh.parts.get(index) else {
            return view(&[] as &[f32], Element::F32, 1);
        };
        let buffers = &part.buffers;
        match field {
            PartField::Positions => view(buffers.positions.as_flattened(), Element::F32, 3),
            PartField::Normals => view(buffers.normals.as_flattened(), Element::F32, 3),
            PartField::Uvs => view(buffers.uvs.as_flattened(), Element::F32, 2),
            PartField::Colors => view(buffers.colors.as_flattened(), Element::F32, 4),
            PartField::Indices => view(&buffers.indices, Element::U32, 3),
        }
    }

    fn graph_view(&self, field: GraphField) -> View<'_> {
        let graph = &self.plant.graph;
        let skeleton = &self.skeleton;
        match field {
            GraphField::Positions => view(skeleton.positions.as_flattened(), Element::F32, 3),
            GraphField::Radii => view(graph.radii(), Element::F32, 1),
            GraphField::Parents => view(&skeleton.parents, Element::I32, 1),
            GraphField::Orders => view(graph.orders(), Element::U8, 1),
            GraphField::Births => view(graph.births(), Element::U16, 1),
            GraphField::Chains => view(&skeleton.chains, Element::I32, 1),
            GraphField::Shed => view(&skeleton.shed, Element::Bool, 1),
            GraphField::OrganNodes => view(&skeleton.organ_nodes, Element::I32, 1),
            GraphField::OrganKinds => view(&skeleton.organ_kinds, Element::U8, 1),
            GraphField::OrganDirections => {
                view(skeleton.organ_directions.as_flattened(), Element::F32, 3)
            }
            GraphField::OrganNormals => {
                view(skeleton.organ_normals.as_flattened(), Element::F32, 3)
            }
            GraphField::OrganScales => view(&skeleton.organ_scales, Element::F32, 1),
            GraphField::OrganBirths => view(&skeleton.organ_births, Element::U16, 1),
        }
    }
}

/// A [`View`] of the `Pod` slice `values`.
fn view<T: bytemuck::Pod>(values: &[T], element: Element, columns: usize) -> View<'_> {
    debug_assert_eq!(std::mem::size_of::<T>(), element.size());
    View {
        bytes: cast_slice(values),
        element,
        columns,
    }
}
