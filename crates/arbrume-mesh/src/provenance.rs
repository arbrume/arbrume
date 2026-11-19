//! Where each vertex of a mesh comes from: the axis or the organ site, and its class.
//!
//! The bark part is filled chain by chain in chain id order, each chain owning one run of
//! vertices; the leaf part is filled instance by instance in the random order of the
//! leaves, every instance with the same number of vertices. Both layouts are known from
//! the planning pass alone, so the provenance of a mesh is computed without building it
//! again and without touching the fill code.
//!
//! In a twig template the stem vertices come first (see `crate::twig`), followed by the
//! fins and the leaves. The stem is labelled [`SemanticClass::Twig`]; the fins, which
//! stand for needles, and the leaves are labelled [`SemanticClass::Leaf`].

use crate::{MeshError, MeshOptions, Mesher};
use arbrume_core::graph::{ChainId, PlantGraph};
use arbrume_core::mesh::{MeshProvenance, PartKind, SemanticClass, VertexRun};
use arbrume_core::species::Species;

impl Mesher {
    /// Provenance of the mesh that [`Mesher::mesh`] builds from the same arguments: one
    /// entry per part, in the same order, with runs that cover its vertices.
    ///
    /// Runs the planning pass only, as [`Mesher::counts`] does.
    ///
    /// # Errors
    ///
    /// The same as [`crate::mesh_tree`].
    pub fn provenance(
        &mut self,
        graph: &PlantGraph,
        species: &Species,
        options: &MeshOptions,
    ) -> Result<MeshProvenance, MeshError> {
        let template = self.plan(graph, species, options)?;
        let mut provenance = MeshProvenance::default();
        if self.bark.indices > 0 {
            let runs = self
                .bark
                .chain_vertices()
                .enumerate()
                .filter(|&(_, (_, len))| len > 0)
                .map(|(chain, (start, len))| {
                    let order = graph.chain(ChainId::new(chain)).order;
                    VertexRun {
                        start: start as u32,
                        len: len as u32,
                        class: axis_class(order),
                        instance: chain as u32,
                    }
                })
                .collect();
            provenance.parts.push((PartKind::Bark, runs));
        }
        if !self.leaves.organs.is_empty() {
            let (per_instance, _) = template.instance_counts();
            let stem = template.stem_vertices.min(per_instance);
            let mut runs = Vec::with_capacity(self.leaves.organs.len() * 2);
            for (slot, &organ) in self.leaves.organs.iter().enumerate() {
                let start = (slot * per_instance) as u32;
                if stem > 0 {
                    runs.push(VertexRun {
                        start,
                        len: stem as u32,
                        class: SemanticClass::Twig,
                        instance: organ,
                    });
                }
                runs.push(VertexRun {
                    start: start + stem as u32,
                    len: (per_instance - stem) as u32,
                    class: SemanticClass::Leaf,
                    instance: organ,
                });
            }
            provenance.parts.push((PartKind::Leaf, runs));
        }
        Ok(provenance)
    }
}

/// Class of the vertices of an axis of branching order `order`.
const fn axis_class(order: u8) -> SemanticClass {
    if order == 0 {
        SemanticClass::Trunk
    } else {
        SemanticClass::Branch
    }
}

/// Provenance of the mesh of `graph`, without building it. See [`Mesher::provenance`].
///
/// # Errors
///
/// The same as [`crate::mesh_tree`].
pub fn mesh_provenance(
    graph: &PlantGraph,
    species: &Species,
    options: &MeshOptions,
) -> Result<MeshProvenance, MeshError> {
    Mesher::new().provenance(graph, species, options)
}
