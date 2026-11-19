//! The branch table: one row per axis in depth-first order, the serialization that
//! Wang et al. (2025) found best for autoregressive tree models.

use arbrume_core::graph::{ChainId, PlantGraph};
use glam::Vec3;

/// Default number of points of the resampled polyline of each branch.
pub const DEFAULT_BRANCH_SAMPLES: usize = 8;

/// One row per branch (axis with at least one live node), in the order in which the
/// depth-first traversal of `PlantGraph::depth_first_order` reaches them, so that a
/// parent row always precedes its children and every subtree is a contiguous run of
/// rows. Shed nodes are left out; an axis shed entirely has no row, nor have its
/// descendants.
///
/// The polyline of a branch runs from its origin (on the centerline of its parent, or
/// the base of a root axis) through the distal ends of its live nodes. `polyline` holds
/// `samples` points evenly spaced along its arc length, the first at the origin and the
/// last at the tip, each `(x, y, z, r)` with the radius of the internode it lies on.
/// `start` and `end` are the first and last of them: the two-endpoint form of Wang et
/// al. (2025).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BranchTable {
    /// Points per polyline.
    pub samples: usize,
    /// Chain id of the axis (the instance id of its bark labels).
    pub chain: Vec<u32>,
    /// Row of the parent branch, `-1` for a root axis.
    pub parent: Vec<i32>,
    /// Arc length of the parent from its origin to the attachment point, divided by the
    /// arc length of the live parent; 0 for a root axis.
    pub attach: Vec<f32>,
    /// Branching order.
    pub order: Vec<u8>,
    /// Growth cycle of the first node.
    pub birth: Vec<u16>,
    /// Number of live nodes.
    pub nodes: Vec<u32>,
    /// Arc length from origin to tip, meters.
    pub length: Vec<f32>,
    /// Radius of the first internode, meters.
    pub base_radius: Vec<f32>,
    /// Radius of the last internode, meters.
    pub tip_radius: Vec<f32>,
    /// Unit direction of the first internode.
    pub direction: Vec<[f32; 3]>,
    /// Origin and base radius `(x, y, z, r)`.
    pub start: Vec<[f32; 4]>,
    /// Tip and tip radius `(x, y, z, r)`.
    pub end: Vec<[f32; 4]>,
    /// `samples` points `(x, y, z, r)` per branch, branch after branch.
    pub polyline: Vec<[f32; 4]>,
}

/// The live part of one axis: origin, distal ends and radii.
struct LiveAxis {
    origin: Vec3,
    points: Vec<Vec3>,
    radii: Vec<f32>,
}

impl LiveAxis {
    fn of(graph: &PlantGraph, chain: ChainId) -> Self {
        let mut nodes: Vec<_> = graph.chain_nodes_rev(chain).collect();
        nodes.reverse();
        let live = nodes.iter().take_while(|node| !graph.is_shed(**node));
        let (points, radii) = live
            .map(|node| (graph.position(*node), graph.radii()[node.index()]))
            .unzip();
        Self {
            origin: graph.chain_origin(chain),
            points,
            radii,
        }
    }

    /// Cumulative arc length at the origin and at every point.
    fn arc(&self) -> Vec<f32> {
        let mut arc = Vec::with_capacity(self.points.len() + 1);
        let mut previous = self.origin;
        let mut total = 0.0;
        arc.push(0.0);
        for &point in &self.points {
            total += point.distance(previous);
            arc.push(total);
            previous = point;
        }
        arc
    }

    /// `count` points evenly spaced by arc length, with their radius.
    fn resample(&self, arc: &[f32], count: usize) -> Vec<[f32; 4]> {
        let total = arc[arc.len() - 1];
        let at = |k: usize| {
            if total <= 0.0 {
                return self.origin.extend(self.radii[0]).to_array();
            }
            let s = total * k as f32 / (count - 1).max(1) as f32;
            let segment = arc.partition_point(|&a| a < s).clamp(1, arc.len() - 1);
            let span = arc[segment] - arc[segment - 1];
            let t = if span > 0.0 {
                ((s - arc[segment - 1]) / span).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let from = if segment == 1 {
                self.origin
            } else {
                self.points[segment - 2]
            };
            let point = from.lerp(self.points[segment - 1], t);
            point.extend(self.radii[segment - 1]).to_array()
        };
        (0..count).map(at).collect()
    }
}

impl BranchTable {
    /// Number of branches.
    #[must_use]
    pub fn len(&self) -> usize {
        self.chain.len()
    }

    /// `true` when there is no branch.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chain.is_empty()
    }

    /// Branches of `graph` with `samples` points per polyline (at least 2).
    ///
    /// # Panics
    ///
    /// Panics when `samples` is below 2.
    #[must_use]
    pub fn from_graph(graph: &PlantGraph, samples: usize) -> Self {
        assert!(samples >= 2, "a branch polyline needs at least 2 points");
        let order = graph.depth_first_order();
        let mut row_of_chain = vec![-1_i32; graph.chain_count()];
        let mut arc_of_chain: Vec<Vec<f32>> = vec![Vec::new(); graph.chain_count()];
        let mut table = Self {
            samples,
            ..Self::default()
        };
        for &node in &order {
            let chain = graph.chain_of(node);
            if graph.chain(chain).first != node || graph.is_shed(node) {
                continue;
            }
            let axis = LiveAxis::of(graph, chain);
            let arc = axis.arc();
            row_of_chain[chain.index()] =
                i32::try_from(table.len()).expect("chain ids are below 2^31");
            table.push(graph, chain, &axis, &arc, &row_of_chain, &arc_of_chain);
            arc_of_chain[chain.index()] = arc;
        }
        table
    }

    /// Appends the row of `chain`, whose parent row is already in.
    fn push(
        &mut self,
        graph: &PlantGraph,
        chain: ChainId,
        axis: &LiveAxis,
        arc: &[f32],
        row_of_chain: &[i32],
        arc_of_chain: &[Vec<f32>],
    ) {
        let attach = graph.chain(chain).attach;
        let (parent, position) = graph.parent_chain(chain).map_or((-1, 0.0), |parent| {
            let parent_arc = &arc_of_chain[parent.index()];
            let rank = graph
                .chain_nodes_rev(parent)
                .position(|node| node == attach)
                .map_or(0, |from_tip| graph.chain(parent).len as usize - from_tip);
            let total = parent_arc[parent_arc.len() - 1];
            let along = parent_arc[rank.min(parent_arc.len() - 1)];
            let ratio = if total > 0.0 { along / total } else { 0.0 };
            (row_of_chain[parent.index()], ratio)
        });
        let polyline = axis.resample(arc, self.samples);
        let first = graph.chain(chain).first;
        self.chain.push(chain.0);
        self.parent.push(parent);
        self.attach.push(position);
        self.order.push(graph.chain(chain).order);
        self.birth.push(graph.births()[first.index()]);
        self.nodes.push(axis.points.len() as u32);
        self.length.push(arc[arc.len() - 1]);
        self.base_radius.push(axis.radii[0]);
        self.tip_radius.push(axis.radii[axis.radii.len() - 1]);
        let direction = (axis.points[0] - axis.origin).normalize_or_zero();
        self.direction.push(direction.to_array());
        self.start.push(polyline[0]);
        self.end.push(polyline[polyline.len() - 1]);
        self.polyline.extend(polyline);
    }
}
