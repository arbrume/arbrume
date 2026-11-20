//! Geometry: turns a grown plant graph and its species into triangle buffers.
//!
//! [`mesh_tree`] produces a [`TreeMesh`] with a bark part (trunk and branches) and a
//! leaf part. The steps, each documented in its module:
//!
//! * `ring`: where the cross-sections of an axis go. The node polyline is smoothed into
//!   a curve, rings follow its curvature, straight runs of nodes are merged, and the base
//!   of the axis is widened (trunk flare, branch collar).
//! * `tube`: the surface over those rings, oriented by a twist-free parallel-transport
//!   frame, with smooth normals, texture coordinates by circumference and arc length,
//!   rings of different side counts zipped together, and a cap on the tip.
//! * `bark`: all axes of the plant, counted first and then filled, in parallel over
//!   chains when asked.
//! * `leaf`, `twig` and `leaves`: the leaf of the species, the twig templates that
//!   carry several leaves on a stem, and their instances at the organ sites, in a random
//!   order that makes any prefix a lower level of detail.
//!
//! Level of detail comes from the graph, not from decimating a mesh: [`MeshOptions`]
//! sets a tolerance that drives ring and side counts, a radius below which axes are
//! dropped (their leaves reattached to a kept ancestor when asked) and the fractions of
//! leaves and of twig leaves kept. [`lod`] plans a chain of such levels within triangle
//! budgets, and [`wind`] computes the baked wind data of a mesh.
//!
//! The output depends on the graph, the species and the options only. In particular it
//! is bit-identical with and without [`MeshOptions::parallel`] and for any number of
//! threads.
//!
//! [`mesh_tree`] is the one-call entry point. A [`Mesher`] does the same while keeping
//! its working memory, and the output buffers if asked, from one plant to the next.
//!
//! ```
//! use arbrume_core::species::Species;
//! use arbrume_mesh::synthetic::{SkeletonParams, skeleton};
//! use arbrume_mesh::{MeshOptions, mesh_tree};
//!
//! let graph = skeleton(&SkeletonParams::small(7));
//! let mesh = mesh_tree(&graph, &Species::default(), &MeshOptions::default()).unwrap();
//! assert_eq!(mesh.parts.len(), 2);
//! let coarse = mesh_tree(&graph, &Species::default(), &MeshOptions::with_quality(0.5)).unwrap();
//! assert!(coarse.triangle_count() < mesh.triangle_count());
//! ```

#![forbid(unsafe_code)]

mod axes;
mod bark;
mod blade_float;
mod junction;
mod leaf;
mod leaves;
pub mod lod;
mod options;
mod pad;
mod provenance;
mod ring;
mod ripple;
pub mod synthetic;
pub mod topology;
mod tube;
mod twig;
pub mod wind;

pub use leaves::LeafInstance;
pub use lod::{LodLevel, LodOptions};
pub use options::{DEFAULT_TOLERANCE, Junctions, MeshError, MeshOptions, OrphanLeaves};
pub use provenance::mesh_provenance;

use arbrume_core::graph::PlantGraph;
use arbrume_core::mesh::{Material, MeshBuffers, MeshPart, PartKind, TreeMesh};
use arbrume_core::species::Species;
use glam::Vec3;
use twig::TemplateSet;

/// Sizes of the mesh [`mesh_tree`] would build, known before any buffer is filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MeshCounts {
    /// Vertices of the bark part.
    pub bark_vertices: usize,
    /// Triangles of the bark part.
    pub bark_triangles: usize,
    /// Instances in the leaf part: leaves, or twigs for a species with a twig.
    pub leaves: usize,
    /// Vertices of the leaf part.
    pub leaf_vertices: usize,
    /// Triangles of the leaf part.
    pub leaf_triangles: usize,
}

impl MeshCounts {
    /// Total number of triangles.
    #[must_use]
    pub const fn triangles(&self) -> usize {
        self.bark_triangles + self.leaf_triangles
    }
}

fn check(graph: &PlantGraph, options: &MeshOptions) -> Result<(), MeshError> {
    options.validate()?;
    if graph.is_contiguous() {
        Ok(())
    } else {
        Err(MeshError::NotContiguous)
    }
}

fn opaque(name: &str, color: [f32; 3], roughness: f32, double_sided: bool) -> Material {
    Material {
        name: name.to_owned(),
        base_color: [color[0], color[1], color[2], 1.0],
        roughness,
        double_sided,
        alpha_cutoff: None,
        base_color_texture: None,
        normal_texture: None,
        roughness_texture: None,
    }
}

/// Removes the part of `kind` from `mesh`, or makes an empty one.
fn take_part(mesh: &mut TreeMesh, kind: PartKind) -> MeshPart {
    let found = mesh.parts.iter().position(|part| part.kind == kind);
    found.map_or_else(
        || MeshPart {
            kind,
            material: opaque("", [0.0; 3], 0.0, false),
            buffers: MeshBuffers::default(),
        },
        |index| mesh.parts.swap_remove(index),
    )
}

/// A reusable mesher: keeps its working memory, and optionally the output buffers,
/// from one plant to the next.
///
/// [`mesh_tree`] allocates its working memory and its output on every call. For a large
/// plant that memory is several megabytes, and obtaining fresh pages from the operating
/// system can cost as much as filling them. A batch that meshes many plants should keep
/// one `Mesher` per thread and, when it consumes each mesh before building the next,
/// one [`TreeMesh`] passed to [`Mesher::mesh_into`].
///
/// The result never depends on what the mesher was used for before.
#[derive(Debug, Clone, Default)]
pub struct Mesher {
    bark: bark::BarkPlan,
    leaves: leaves::LeafSet,
    circles: tube::Circles,
    /// Whether each chain is meshed, by chain id.
    kept: Vec<bool>,
    /// Arc length of each node along its axis.
    arcs: Vec<f32>,
    wind: wind::WindPlan,
    /// Largest radius cut of the level-of-detail chain being planned.
    radius_cap: f32,
}

impl Mesher {
    /// A mesher with no memory reserved yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs the planning passes and returns the foliage templates.
    fn plan(
        &mut self,
        graph: &PlantGraph,
        species: &Species,
        options: &MeshOptions,
    ) -> Result<TemplateSet, MeshError> {
        check(graph, options)?;
        bark::plan_bark(&mut self.bark, graph, species, options)?;
        self.bark.fill_kept(&mut self.kept);
        let reattach = options.orphan_leaves == OrphanLeaves::Reattach && options.min_radius > 0.0;
        if reattach || options.wind {
            axes::node_arcs(graph, &mut self.arcs);
        }
        let mounting = axes::Mounting {
            graph,
            arcs: &self.arcs,
            kept: &self.kept,
            max_cycle: options.max_cycle,
        };
        leaves::select_leaves(&mut self.leaves, &mounting, &species.foliage, options);
        let (foliage, bark) = (&species.foliage, &species.bark);
        let (tolerance, seed) = (options.tolerance, options.seed);
        let fraction = options.twig_leaf_fraction;
        let mut template = TemplateSet::thinned(foliage, bark, tolerance, seed, fraction);
        blade_float::add_blade_floats(&mut template, species);
        let (vertices, _) = leaves::leaf_counts(&template, self.leaves.organs.len());
        if u32::try_from(vertices).is_err() {
            return Err(MeshError::TooLarge { vertices });
        }
        Ok(template)
    }

    /// Same as [`mesh_counts`], reusing the working memory of the mesher.
    ///
    /// # Errors
    ///
    /// The same as [`mesh_tree`].
    pub fn counts(
        &mut self,
        graph: &PlantGraph,
        species: &Species,
        options: &MeshOptions,
    ) -> Result<MeshCounts, MeshError> {
        let template = self.plan(graph, species, options)?;
        let leaves = self.leaves.organs.len();
        let (leaf_vertices, leaf_indices) = leaves::leaf_counts(&template, leaves);
        Ok(MeshCounts {
            bark_vertices: self.bark.vertices,
            bark_triangles: self.bark.indices / 3,
            leaves,
            leaf_vertices,
            leaf_triangles: leaf_indices / 3,
        })
    }

    /// Same as [`mesh_tree`], reusing the working memory of the mesher.
    ///
    /// # Errors
    ///
    /// The same as [`mesh_tree`].
    pub fn mesh(
        &mut self,
        graph: &PlantGraph,
        species: &Species,
        options: &MeshOptions,
    ) -> Result<TreeMesh, MeshError> {
        let mut mesh = TreeMesh::default();
        self.mesh_into(graph, species, options, &mut mesh)?;
        Ok(mesh)
    }

    /// Same as [`mesh_tree`], writing into `mesh` and reusing the allocations of its
    /// bark and leaf parts. Whatever `mesh` held is replaced; the result equals that of
    /// [`mesh_tree`].
    ///
    /// # Errors
    ///
    /// The same as [`mesh_tree`]. On error `mesh` is left untouched.
    pub fn mesh_into(
        &mut self,
        graph: &PlantGraph,
        species: &Species,
        options: &MeshOptions,
        mesh: &mut TreeMesh,
    ) -> Result<(), MeshError> {
        let template = self.plan(graph, species, options)?;
        let mut bark = take_part(mesh, PartKind::Bark);
        let mut leaf = take_part(mesh, PartKind::Leaf);
        mesh.parts.clear();
        mesh.wind = None;
        self.wind.leaves.clear();
        if options.wind {
            self.wind
                .build(graph, &self.bark, &self.arcs, options.max_cycle);
            let (organs, mounts) = (&self.leaves.organs, &self.leaves.mounts);
            self.wind.fill_leaves(graph, organs, mounts, &self.arcs);
            mesh.wind = Some(self.wind.table.clone());
        }
        if self.bark.indices > 0 {
            let color = species.bark.material_color();
            bark.material = opaque("bark", color, species.bark.roughness, false);
            let circles = &mut self.circles;
            bark::fill_bark(&self.bark, circles, options.parallel, &mut bark.buffers);
            if options.wind {
                self.wind.fill_bark(&self.bark, &mut bark.buffers);
            } else {
                bark.buffers.wind.clear();
            }
            mesh.parts.push(bark);
        }
        let foliage = &species.foliage;
        if !self.leaves.organs.is_empty() {
            leaf.material = opaque("leaf", template.base_color, foliage.roughness, true);
            leaf.material.alpha_cutoff = foliage.uses_cards().then_some(0.5);
            let source = leaves::LeafSource {
                graph,
                foliage,
                set: &self.leaves,
                templates: &template,
                wind: &self.wind.leaves,
            };
            leaves::mesh_leaves(&source, options, &mut leaf.buffers);
            mesh.parts.push(leaf);
        }
        Ok(())
    }
}

/// The leaf part of a mesh as instances of a few templates, for exporters that instance
/// geometry instead of baking it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LeafInstances {
    /// Template variants in the frame of a leaf (unit: one leaf length), with their
    /// normals, texture coordinates and, as RGBA colours, the tints that multiply the
    /// colour of an instance.
    pub templates: Vec<MeshBuffers>,
    /// Base colour of the leaf material.
    pub base_color: [f32; 3],
    /// The instances, in the order of the leaf part.
    pub instances: Vec<LeafInstance>,
}

impl Mesher {
    /// The leaves that [`Mesher::mesh`] builds from the same arguments, as instances of
    /// their templates: transforming template `variant` of each instance gives the leaf
    /// part vertex for vertex.
    ///
    /// # Errors
    ///
    /// The same as [`mesh_tree`].
    pub fn leaf_instances(
        &mut self,
        graph: &PlantGraph,
        species: &Species,
        options: &MeshOptions,
    ) -> Result<LeafInstances, MeshError> {
        let template = self.plan(graph, species, options)?;
        let source = leaves::LeafSource {
            graph,
            foliage: &species.foliage,
            set: &self.leaves,
            templates: &template,
            wind: &[],
        };
        let instances = leaves::leaf_instances(&source, options);
        let templates = template
            .variants
            .iter()
            .map(|variant| MeshBuffers {
                positions: variant.positions.iter().map(Vec3::to_array).collect(),
                normals: variant.normals.iter().map(Vec3::to_array).collect(),
                uvs: variant.uvs.clone(),
                colors: variant
                    .tints
                    .iter()
                    .map(|t| [t[0], t[1], t[2], 1.0])
                    .collect(),
                wind: Vec::new(),
                indices: variant.indices.clone(),
            })
            .collect();
        Ok(LeafInstances {
            templates,
            base_color: template.base_color,
            instances,
        })
    }
}

/// Counts the vertices, triangles and leaves of the mesh of `graph` without building it.
///
/// This runs the planning pass of [`mesh_tree`] only, a fraction of its cost. Use it to
/// budget a level of detail or to size buffers in advance.
///
/// # Errors
///
/// The same as [`mesh_tree`].
pub fn mesh_counts(
    graph: &PlantGraph,
    species: &Species,
    options: &MeshOptions,
) -> Result<MeshCounts, MeshError> {
    Mesher::new().counts(graph, species, options)
}

/// Builds the mesh of a plant.
///
/// The result has a [`PartKind::Bark`] part, with normals and texture coordinates,
/// followed by a [`PartKind::Leaf`] part, with normals, texture coordinates and vertex
/// colours; a part that would be empty is left out. The bark part has vertex colours
/// when the species varies the colour of its bark (`bark.young_color`,
/// `bark.color_variation`). Materials take their colour and roughness from
/// `species.bark` and `species.foliage`, carry no texture, and the leaf material is
/// double-sided. The leaf part also holds the stems of twigs (`foliage.twig`). For card leaves (`foliage.geometry = "card"`) the leaf
/// material is alpha-masked; the caller attaches the texture.
///
/// Organ sites of a kind other than `Leaf` are ignored.
///
/// The graph must be contiguous: every chain must be one slice of the node arrays. A
/// generator that completes each axis before starting the next produces such a graph;
/// otherwise call `PlantGraph::ensure_contiguous` first. Organ sites must hold unit,
/// orthogonal `direction` and `normal` vectors.
///
/// To mesh many plants, use a [`Mesher`], which does not allocate anew for each.
///
/// # Errors
///
/// Returns [`MeshError::NotContiguous`] for a graph that is not contiguous,
/// [`MeshError::InvalidOption`] for an option outside its domain and
/// [`MeshError::TooLarge`] when a part would need more vertices than `u32` indices can
/// address.
pub fn mesh_tree(
    graph: &PlantGraph,
    species: &Species,
    options: &MeshOptions,
) -> Result<TreeMesh, MeshError> {
    Mesher::new().mesh(graph, species, options)
}
