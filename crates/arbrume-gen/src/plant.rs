//! A generated plant and its summary statistics.

use std::time::Duration;

use arbrume_core::graph::{OrganKind, PlantGraph};
use arbrume_core::mesh::TreeMesh;
use serde::Serialize;

use crate::params::GenParams;

/// Axis-aligned bounding box, in meters, Y up.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Bounds {
    /// Smallest coordinates.
    pub min: [f32; 3],
    /// Largest coordinates.
    pub max: [f32; 3],
}

impl Bounds {
    /// Size along each axis.
    #[must_use]
    pub fn size(&self) -> [f32; 3] {
        std::array::from_fn(|axis| self.max[axis] - self.min[axis])
    }
}

/// Summary of a generated plant.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct Stats {
    /// Nodes (metamers) of the graph.
    pub nodes: usize,
    /// Chains (axes) of the graph.
    pub chains: usize,
    /// Leaf sites of the graph. The mesh may hold fewer leaves (`leaf_fraction`).
    pub leaf_sites: usize,
    /// Vertices of the mesh, all parts.
    pub vertices: usize,
    /// Triangles of the mesh, all parts.
    pub triangles: usize,
    /// Bounding box of the mesh vertices, `None` for an empty mesh.
    pub bounds: Option<Bounds>,
    /// Vertical extent of the mesh in meters (0 for an empty mesh).
    pub height: f32,
}

impl Stats {
    /// Statistics of a graph and of its mesh.
    #[must_use]
    pub fn of(graph: &PlantGraph, mesh: &TreeMesh) -> Self {
        let mut stats = Self {
            nodes: graph.node_count(),
            chains: graph.chain_count(),
            leaf_sites: graph
                .organs()
                .iter()
                .filter(|site| site.kind == OrganKind::Leaf)
                .count(),
            ..Self::default()
        };
        stats.set_mesh(mesh);
        stats
    }

    /// Replaces the mesh statistics (vertices, triangles, bounds, height) by those of
    /// `mesh`.
    pub fn set_mesh(&mut self, mesh: &TreeMesh) {
        self.vertices = mesh.parts.iter().map(|p| p.buffers.positions.len()).sum();
        self.triangles = mesh.triangle_count();
        self.bounds = mesh_bounds(mesh);
        self.height = self.bounds.map_or(0.0, |b| b.size()[1]);
    }
}

/// Bounding box of every vertex of `mesh`.
fn mesh_bounds(mesh: &TreeMesh) -> Option<Bounds> {
    let mut points = mesh.parts.iter().flat_map(|p| p.buffers.positions.iter());
    let first = *points.next()?;
    let mut bounds = Bounds {
        min: first,
        max: first,
    };
    for point in points {
        for axis in 0..3 {
            bounds.min[axis] = bounds.min[axis].min(point[axis]);
            bounds.max[axis] = bounds.max[axis].max(point[axis]);
        }
    }
    Some(bounds)
}

/// A generated plant: its parameters, skeleton, mesh and statistics.
///
/// The graph holds the skeleton (node positions, radii, parents, chains, orders, births)
/// and the organ sites, everything a skeleton or annotation exporter needs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plant {
    /// Parameters the plant was generated with; `mesh` holds the options of the current
    /// mesh.
    pub params: GenParams,
    /// The grown plant graph.
    pub graph: PlantGraph,
    /// The mesh of the graph.
    pub mesh: TreeMesh,
    /// Statistics of the graph and of the mesh.
    pub stats: Stats,
}

/// Wall-clock time of the stages of one generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Timings {
    /// Growth of the graph.
    pub grow: Duration,
    /// Meshing.
    pub mesh: Duration,
}
