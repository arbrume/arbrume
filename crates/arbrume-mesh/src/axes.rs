//! Positions along the axes of a plant: arc lengths of the nodes, and the place where a
//! leaf whose own axis is not meshed is attached instead.
//!
//! The arc length of a node is measured from the origin of its axis along the polyline
//! `origin, positions[0], positions[1], ...`, the measure the rings of the bark use, so a
//! position along an axis means the same thing to the bark, the leaves and the wind data.
//!
//! **Reattachment.** When [`MeshOptions::min_radius`](crate::MeshOptions) drops an axis,
//! the leaves it bears would float where it was. With
//! [`OrphanLeaves::Reattach`](crate::OrphanLeaves) each such leaf goes to its nearest
//! kept ancestor: the lineage of the leaf is followed down to the first axis that is
//! meshed, and the leaf moves to the point of that axis nearest to its own node, looked
//! for within [`WINDOW`] internodes of the node the dropped lineage grows from. The leaf
//! then sits on the surface of that axis, as any leaf sits on its own.

use arbrume_core::graph::{ChainId, NodeId, PlantGraph};
use glam::Vec3;

/// Internodes searched on each side of the node a dropped lineage grows from.
const WINDOW: usize = 8;

/// Fills `out` with the arc length of every node from the origin of its axis. The graph
/// must be contiguous.
pub(crate) fn node_arcs(graph: &PlantGraph, out: &mut Vec<f32>) {
    out.clear();
    out.resize(graph.node_count(), 0.0);
    for chain in graph.chain_ids() {
        let Some(view) = graph.chain_view(chain) else {
            continue;
        };
        let first = view.first.index();
        let mut previous = view.origin;
        let mut arc = 0.0;
        for (offset, &position) in view.positions.iter().enumerate() {
            arc += position.distance(previous);
            out[first + offset] = arc;
            previous = position;
        }
    }
}

/// Where a leaf is attached: a point of the centerline of a meshed axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Mount {
    /// Point of the centerline.
    pub center: Vec3,
    /// Unit direction of the centerline there.
    pub axis: Vec3,
    /// Radius of the axis there.
    pub radius: f32,
    /// Chain id of the axis.
    pub chain: u32,
    /// Arc length of the point from the origin of the axis.
    pub arc: f32,
}

/// What [`mount`] reads.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Mounting<'a> {
    pub graph: &'a PlantGraph,
    /// Arc length of every node, from [`node_arcs`].
    pub arcs: &'a [f32],
    /// Whether each chain is meshed, indexed by chain id.
    pub kept: &'a [bool],
    /// Age cut of the mesh.
    pub max_cycle: Option<u16>,
}

impl Mounting<'_> {
    /// The mount of a leaf on `node`: on its own axis when that axis is meshed,
    /// otherwise on the nearest kept ancestor. `None` when no ancestor is kept.
    pub(crate) fn mount(&self, node: NodeId) -> Option<Mount> {
        let graph = self.graph;
        let (mut at, mut chain) = (node, graph.chain_of(node));
        while !self.kept[chain.index()] {
            at = graph.chain(chain).attach;
            if at.is_none() {
                return None;
            }
            chain = graph.chain_of(at);
        }
        if at == node {
            return Some(self.on_node(node, chain));
        }
        Some(self.nearest(chain, at, graph.position(node)))
    }

    /// The mount at the node `node` of its own axis `chain`.
    pub(crate) fn on_node(&self, node: NodeId, chain: ChainId) -> Mount {
        let graph = self.graph;
        let center = graph.position(node);
        Mount {
            center,
            axis: (center - graph.proximal(node)).normalize_or_zero(),
            radius: graph.radii()[node.index()],
            chain: chain.0,
            arc: self.arcs[node.index()],
        }
    }

    /// The point of `chain` nearest to `target` within [`WINDOW`] internodes of `at`.
    fn nearest(&self, chain: ChainId, at: NodeId, target: Vec3) -> Mount {
        let graph = self.graph;
        let view = graph.chain_view(chain).expect("contiguous graph");
        let kept = self
            .max_cycle
            .map_or(view.len(), |cycle| view.born_by(cycle))
            .max(1);
        let first = view.first.index();
        let index = at.index().saturating_sub(first).min(kept - 1);
        let point = |i: usize| {
            if i == 0 {
                view.origin
            } else {
                view.positions[i - 1]
            }
        };
        let radius = |i: usize| view.radii[i.saturating_sub(1)];
        let arc = |i: usize| {
            if i == 0 {
                0.0
            } else {
                self.arcs[first + i - 1]
            }
        };
        let (low, high) = (index.saturating_sub(WINDOW), (index + 1 + WINDOW).min(kept));
        let mut best = (f32::INFINITY, 0, 0.0);
        for segment in low..high {
            let (a, b) = (point(segment), point(segment + 1));
            let span = b - a;
            let t = (target - a).dot(span) / span.length_squared().max(1e-12);
            let t = t.clamp(0.0, 1.0);
            let distance = target.distance_squared(a + span * t);
            if distance < best.0 {
                best = (distance, segment, t);
            }
        }
        let (_, segment, t) = best;
        let (a, b) = (point(segment), point(segment + 1));
        Mount {
            center: a.lerp(b, t),
            axis: (b - a).normalize_or_zero(),
            radius: radius(segment) + (radius(segment + 1) - radius(segment)) * t,
            chain: chain.0,
            arc: arc(segment) + (arc(segment + 1) - arc(segment)) * t,
        }
    }
}
