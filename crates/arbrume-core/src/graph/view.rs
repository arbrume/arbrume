//! Read-only views for the mesher: one chain as slices, and bounds.

use super::{Chain, ChainId, NodeId, PlantGraph};
use glam::Vec3;

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    /// Smallest corner.
    pub min: Vec3,
    /// Largest corner.
    pub max: Vec3,
}

impl Aabb {
    /// Box containing the single point `point`.
    #[must_use]
    pub const fn from_point(point: Vec3) -> Self {
        Self {
            min: point,
            max: point,
        }
    }

    /// Smallest box containing `self` and `point`.
    #[must_use]
    pub fn including(self, point: Vec3) -> Self {
        Self {
            min: self.min.min(point),
            max: self.max.max(point),
        }
    }

    /// Box grown by `margin` on every side.
    #[must_use]
    pub fn expanded(self, margin: f32) -> Self {
        Self {
            min: self.min - Vec3::splat(margin),
            max: self.max + Vec3::splat(margin),
        }
    }

    /// Edge lengths.
    #[must_use]
    pub fn size(self) -> Vec3 {
        self.max - self.min
    }

    /// Center point.
    #[must_use]
    pub fn center(self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// `true` when `point` is inside or on the boundary.
    #[must_use]
    pub fn contains(self, point: Vec3) -> bool {
        point.cmpge(self.min).all() && point.cmple(self.max).all()
    }
}

/// One chain laid out for sweeping a generalized cylinder.
///
/// The axis is the polyline `origin, positions[0], positions[1], ...`. `radii[i]` is the
/// radius of the internode ending at `positions[i]`. Shed nodes, which are always at the
/// distal end of a chain, are left out, so the view can be empty.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChainView<'a> {
    /// The chain viewed.
    pub chain: ChainId,
    /// Branching order, 0 for the trunk.
    pub order: u8,
    /// Node of the parent chain the axis starts from; [`NodeId::NONE`] for a root chain.
    pub attach: NodeId,
    /// Start of the axis: the position of `attach`, on the centerline of the parent, or
    /// the base point of a root chain.
    pub origin: Vec3,
    /// Radius of the parent internode at `attach`, `None` for a root chain. A junction
    /// collar starts about this far from `origin`.
    pub parent_radius: Option<f32>,
    /// Id of the node at `positions[0]`; node `i` of the view is `first.0 + i`.
    pub first: NodeId,
    /// Distal positions of the live nodes, base to tip.
    pub positions: &'a [Vec3],
    /// Internode radii of the live nodes.
    pub radii: &'a [f32],
    /// Birth cycles of the live nodes, never decreasing.
    pub births: &'a [u16],
}

impl ChainView<'_> {
    /// Number of live nodes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.positions.len()
    }

    /// `true` when the chain has no live node.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    /// Id of node `i` of the view.
    #[must_use]
    pub const fn node(&self, i: usize) -> NodeId {
        NodeId::new(self.first.index() + i)
    }

    /// Number of leading nodes born at or before `cycle`: the chain as it was at that
    /// growth cycle is the first `born_by(cycle)` nodes.
    #[must_use]
    pub fn born_by(&self, cycle: u16) -> usize {
        self.births.partition_point(|&birth| birth <= cycle)
    }

    /// Arc length from the origin to the last live node.
    #[must_use]
    pub fn length(&self) -> f32 {
        let mut previous = self.origin;
        let mut total = 0.0;
        for &position in self.positions {
            total += position.distance(previous);
            previous = position;
        }
        total
    }
}

impl PlantGraph {
    /// View of the live part of `chain`. `None` when the layout is not contiguous; call
    /// [`PlantGraph::ensure_contiguous`] first.
    #[must_use]
    pub fn chain_view(&self, chain: ChainId) -> Option<ChainView<'_>> {
        let range = self.chain_range(chain)?;
        let Chain {
            first,
            attach,
            order,
            ..
        } = self.chains[chain.index()];
        let live = self.flags[range.clone()].partition_point(|flags| !flags.is_shed());
        let range = range.start..range.start + live;
        Some(ChainView {
            chain,
            order,
            attach,
            origin: self.chain_origin(chain),
            parent_radius: attach.is_some().then(|| self.radius[attach.index()]),
            first,
            positions: &self.position[range.clone()],
            radii: &self.radius[range.clone()],
            births: &self.birth[range],
        })
    }

    /// Bounding box of the axis points: all node positions and root chain origins, shed
    /// nodes included, radii and organs not included. `None` for an empty graph.
    #[must_use]
    pub fn bounds(&self) -> Option<Aabb> {
        let (&first, rest) = self.position.split_first()?;
        let nodes = rest
            .iter()
            .fold(Aabb::from_point(first), |aabb, &p| aabb.including(p));
        let roots = self.chains.iter().filter(|chain| chain.attach.is_none());
        Some(roots.fold(nodes, |aabb, chain| aabb.including(chain.origin)))
    }
}

#[cfg(test)]
mod tests {
    use crate::graph::{Aabb, ChainKind, Metamer, NodeId, PlantGraph};
    use glam::Vec3;

    #[test]
    fn chain_view_exposes_the_axis_and_its_attachment() {
        let mut graph = PlantGraph::new();
        let base =
            graph.add_root_chain(Vec3::new(0.0, -1.0, 0.0), Metamer::new(Vec3::ZERO, 0.3, 0));
        let trunk = graph.chain_of(base);
        graph.extend_chain(trunk, Metamer::new(Vec3::Y, 0.2, 1));
        let side = graph.add_chain(base, ChainKind::Lateral, Metamer::new(Vec3::X, 0.1, 1));
        graph.extend_chain(graph.chain_of(side), Metamer::new(Vec3::X * 3.0, 0.05, 4));

        let trunk_view = graph.chain_view(trunk).unwrap();
        assert_eq!(trunk_view.origin, Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(trunk_view.parent_radius, None);
        assert_eq!(trunk_view.positions, [Vec3::ZERO, Vec3::Y]);
        assert!((trunk_view.length() - 2.0).abs() < 1e-6);

        let view = graph.chain_view(graph.chain_of(side)).unwrap();
        assert_eq!(
            (view.order, view.attach, view.origin),
            (1, base, Vec3::ZERO)
        );
        assert_eq!(
            view.parent_radius.map(f32::to_bits),
            Some(0.3_f32.to_bits())
        );
        assert_eq!(view.radii.len(), 2);
        assert_eq!(view.node(1), NodeId::new(3));
        assert_eq!(
            (view.born_by(0), view.born_by(1), view.born_by(4)),
            (0, 1, 2)
        );
        assert!((view.length() - 3.0).abs() < 1e-6);
    }

    #[test]
    fn bounds_cover_nodes_and_root_origins() {
        assert_eq!(PlantGraph::new().bounds(), None);
        let mut graph = PlantGraph::new();
        let base = graph.add_root_chain(Vec3::new(0.0, -1.0, 0.0), Metamer::new(Vec3::Y, 0.3, 0));
        graph.add_chain(
            base,
            ChainKind::Lateral,
            Metamer::new(Vec3::new(2.0, 0.5, -1.0), 0.1, 0),
        );
        let bounds = graph.bounds().unwrap();
        assert_eq!(
            bounds,
            Aabb {
                min: Vec3::new(0.0, -1.0, -1.0),
                max: Vec3::new(2.0, 1.0, 0.0)
            }
        );
        assert!(bounds.contains(bounds.center()) && !bounds.contains(Vec3::splat(5.0)));
        assert_eq!(
            bounds.expanded(1.0).size(),
            bounds.size() + Vec3::splat(2.0)
        );
    }
}
