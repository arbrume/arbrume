//! Hand-built skeletons for tests, benchmarks and demos.
//!
//! [`skeleton`] builds a plausible branching [`PlantGraph`] with leaf sites directly
//! through the graph API, without any growth model: a wandering trunk, laterals placed
//! by a divergence angle whose length shrinks towards the tip of their parent, pipe-model
//! radii and one leaf per node on the last two orders. It exists so that the mesher can
//! be exercised on trees of any size; it is not a model of any plant, and its parameters
//! are arbitrary.
//!
//! The result is contiguous (each axis is completed before the next is started) and is
//! a function of the parameters only.

use arbrume_core::graph::{ChainKind, Metamer, NodeId, OrganKind, OrganSite, PlantGraph};
use arbrume_core::rng::{Rng, Stream};
use glam::Vec3;

/// Divergence angle between successive laterals and leaves, in radians (137.5 degrees).
const DIVERGENCE: f32 = 2.399_827_7;

/// Parameters of [`skeleton`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkeletonParams {
    /// Seed of every random draw.
    pub seed: u64,
    /// Number of nodes of the trunk.
    pub trunk_nodes: u32,
    /// Internode length of the trunk, in meters.
    pub trunk_internode: f32,
    /// Highest branching order.
    pub max_order: u8,
    /// Probability that a node bears a lateral axis.
    pub branch_probability: f32,
    /// Node count of a lateral at the base of its parent, as a fraction of the node
    /// count of the parent; laterals nearer the tip are shorter.
    pub length_ratio: f32,
    /// Internode length of a lateral divided by that of its parent.
    pub internode_ratio: f32,
    /// Angle between a lateral and its parent, in degrees.
    pub branching_angle_deg: f32,
    /// Random change of direction at each node, as a fraction of a unit step.
    pub wander: f32,
    /// Whether leaf sites are added.
    pub leaves: bool,
}

impl Default for SkeletonParams {
    fn default() -> Self {
        Self {
            seed: 1,
            trunk_nodes: 40,
            trunk_internode: 0.25,
            max_order: 3,
            branch_probability: 0.6,
            length_ratio: 0.5,
            internode_ratio: 0.8,
            branching_angle_deg: 55.0,
            wander: 0.12,
            leaves: true,
        }
    }
}

impl SkeletonParams {
    /// A small tree of about a thousand nodes.
    #[must_use]
    pub fn small(seed: u64) -> Self {
        Self {
            seed,
            ..Self::default()
        }
    }

    /// A tree of about ten thousand nodes.
    #[must_use]
    pub fn medium(seed: u64) -> Self {
        Self {
            seed,
            trunk_nodes: 52,
            max_order: 4,
            branch_probability: 0.62,
            length_ratio: 0.57,
            ..Self::default()
        }
    }

    /// A tree of about a hundred thousand nodes.
    #[must_use]
    pub fn large(seed: u64) -> Self {
        Self {
            seed,
            trunk_nodes: 66,
            max_order: 5,
            branch_probability: 0.66,
            length_ratio: 0.63,
            ..Self::default()
        }
    }
}

/// An axis waiting to be built.
#[derive(Debug, Clone, Copy)]
struct Shoot {
    /// Node the axis starts from; `NONE` for the trunk.
    attach: NodeId,
    start: Vec3,
    direction: Vec3,
    nodes: u32,
    internode: f32,
    order: u8,
    birth: u16,
}

struct Builder<'a> {
    params: &'a SkeletonParams,
    graph: PlantGraph,
    pending: Vec<Shoot>,
}

impl Builder<'_> {
    /// Random cursor of the node about to be created, for the decision `stream`.
    fn rng(&self, stream: u16) -> Rng {
        Rng::new(
            self.params.seed,
            self.graph.node_count() as u32,
            Stream::custom(stream),
        )
    }

    /// Builds one axis node by node, then queues its laterals.
    fn grow(&mut self, shoot: &Shoot) {
        let params = self.params;
        let lift = if shoot.order == 0 { 0.25 } else { 0.04 };
        let (mut position, mut direction) = (shoot.start, shoot.direction);
        let mut chain = None;
        for index in 0..shoot.nodes {
            let turn = self.rng(0).unit_sphere() * params.wander;
            direction = (direction + turn + Vec3::Y * lift).normalize();
            position += direction * shoot.internode;
            let birth = shoot.birth.saturating_add((index / 4) as u16);
            let metamer = Metamer::new(position, 0.003, birth);
            let node = match chain {
                Some(chain) => self.graph.extend_chain(chain, metamer),
                None if shoot.attach.is_none() => self.graph.add_root_chain(shoot.start, metamer),
                None => self
                    .graph
                    .add_chain(shoot.attach, ChainKind::Lateral, metamer),
            };
            chain = Some(self.graph.chain_of(node));
            let place = Place {
                node,
                position,
                direction,
                index,
                birth,
            };
            if params.leaves && shoot.order + 1 >= params.max_order {
                self.leaf(&place, shoot);
            }
            if index + 1 < shoot.nodes {
                self.branch(&place, shoot);
            }
        }
    }

    /// Unit direction at `angle` from `axis`, at the azimuth of node `index`.
    fn lateral_direction(&self, axis: Vec3, index: u32, angle: f32) -> Vec3 {
        let azimuth = index as f32 * DIVERGENCE + self.rng(1).signed() * 0.3;
        let (u, v) = axis.any_orthonormal_pair();
        let radial = u * azimuth.cos() + v * azimuth.sin();
        axis * angle.cos() + radial * angle.sin()
    }

    /// Queues a lateral axis at `place` when the draw says so.
    fn branch(&mut self, place: &Place, shoot: &Shoot) {
        let params = self.params;
        let along = (place.index + 1) as f32 / shoot.nodes as f32;
        let bare = if shoot.order == 0 { 0.25 } else { 0.15 };
        let mut rng = self.rng(2);
        if shoot.order >= params.max_order || along < bare || !rng.chance(params.branch_probability)
        {
            return;
        }
        let nodes = shoot.nodes as f32 * params.length_ratio * (1.2 - along);
        let angle = (params.branching_angle_deg + 10.0 * rng.signed()).to_radians();
        self.pending.push(Shoot {
            attach: place.node,
            start: place.position,
            direction: self.lateral_direction(place.direction, place.index, angle),
            nodes: (nodes.round() as u32).max(2),
            internode: shoot.internode * params.internode_ratio,
            order: shoot.order + 1,
            birth: place.birth.saturating_add(1),
        });
    }

    /// Adds a leaf site at `place`.
    fn leaf(&mut self, place: &Place, shoot: &Shoot) {
        let direction = self.lateral_direction(place.direction, place.index, 0.9);
        let up = Vec3::Y.reject_from_normalized(direction);
        let normal = up.normalize_or(direction.any_orthonormal_vector());
        let along = (place.index + 1) as f32 / shoot.nodes as f32;
        self.graph.add_organ(OrganSite {
            node: place.node,
            kind: OrganKind::Leaf,
            direction,
            normal,
            scale: 0.7 + 0.3 * along,
            birth: place.birth,
        });
    }
}

/// A node just created on an axis.
#[derive(Debug, Clone, Copy)]
struct Place {
    node: NodeId,
    position: Vec3,
    direction: Vec3,
    /// Rank of the node on its axis.
    index: u32,
    birth: u16,
}

/// Builds a contiguous tree skeleton with pipe-model radii and, when asked, leaf sites.
#[must_use]
pub fn skeleton(params: &SkeletonParams) -> PlantGraph {
    let mut builder = Builder {
        params,
        graph: PlantGraph::new(),
        pending: vec![Shoot {
            attach: NodeId::NONE,
            start: Vec3::ZERO,
            direction: Vec3::Y,
            nodes: params.trunk_nodes.max(1),
            internode: params.trunk_internode,
            order: 0,
            birth: 0,
        }],
    };
    while let Some(shoot) = builder.pending.pop() {
        builder.grow(&shoot);
    }
    let mut graph = builder.graph;
    graph.pipe_radii(2.3, 0.003);
    graph
}

#[cfg(test)]
mod tests {
    use super::{SkeletonParams, skeleton};

    #[test]
    fn skeletons_are_valid_contiguous_and_reproducible() {
        for seed in 0..8 {
            let params = SkeletonParams::small(seed);
            let graph = skeleton(&params);
            graph.check_invariants().unwrap();
            assert!(graph.is_contiguous());
            assert!(graph.chain_count() > 10 && !graph.organs().is_empty());
            assert_eq!(graph, skeleton(&params));
        }
        assert_ne!(
            skeleton(&SkeletonParams::small(1)),
            skeleton(&SkeletonParams::small(2))
        );
    }

    #[test]
    fn presets_have_about_the_advertised_size() {
        let small = skeleton(&SkeletonParams::small(1)).node_count();
        let medium = skeleton(&SkeletonParams::medium(1)).node_count();
        assert!((500..2_000).contains(&small), "{small}");
        assert!((5_000..20_000).contains(&medium), "{medium}");
    }
}
