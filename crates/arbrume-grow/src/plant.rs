//! The growing plant: the graph plus the state of every meristem.
//!
//! A meristem is the growing point that builds one axis. It exists from the moment its
//! bud is formed, rests until the bud fate stage lets it flush, then extends its chain
//! cycle after cycle until it is done.
//!
//! Every meristem has a 64-bit `key` derived from the seed of the plant and from its
//! path: the stem number, then for each ancestor the index of the bearing metamer and the
//! slot of the bud on it. The key seeds every random draw made for the meristem, so the
//! draws depend neither on node ids, which change in compaction, nor on meristem ids,
//! which shift when a stage admits a different set of buds at another age.

use arbrume_core::graph::{BudId, ChainId, NodeId, PlantGraph};
use arbrume_core::rng::{Rng, Stream, mix64};
use glam::Vec3;

/// Identifier of a meristem: its index in [`Plant::meristems`]. Stable for the life of
/// the plant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MeristemId(pub u32);

impl MeristemId {
    /// Index in [`Plant::meristems`].
    #[inline]
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Where a meristem comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SiteKind {
    /// The seed of a stem rising from the ground (a root chain of order 0).
    Seed,
    /// An axillary bud: its axis has the order of the bearing axis plus one.
    Lateral,
    /// A bud continuing its bearing axis with the same order: the relay of a sympodial
    /// axis or the second axis of a fork.
    Relay,
}

/// Activity of a meristem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeristemState {
    /// Formed but not yet flushed; it has no chain.
    Resting,
    /// Building its chain.
    Growing,
    /// Its apex died and a relay continues the axis (sympodial growth).
    Relayed,
    /// Its axis is finished; it never grows again.
    Done,
}

/// State of one meristem and of the axis it builds.
///
/// Lengths are measured along the *apparent axis*: a relay continues the arc length of
/// the axis it relays, so `length` starts at `start` instead of 0 and relative positions
/// keep increasing across relays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meristem {
    /// Seed of the random draws of this meristem. See the [module documentation](self).
    pub key: u64,
    /// Origin of the meristem.
    pub kind: SiteKind,
    /// Meristem of the bearing axis; `None` for a seed.
    pub parent: Option<MeristemId>,
    /// Activity.
    pub state: MeristemState,
    /// Branching order of its axis.
    pub order: u8,
    /// Growth cycle in which the meristem was formed.
    pub born: u16,
    /// Its bud in the graph; [`BudId::NONE`] for a seed that has not flushed.
    pub bud: BudId,
    /// Its chain; [`ChainId::NONE`] until the first flush.
    pub chain: ChainId,
    /// Point from which the next metamer extends.
    pub tip: Vec3,
    /// Unit growth direction.
    pub heading: Vec3,
    /// Unit vector orthogonal to `heading`, transported along the axis; the reference
    /// of phyllotactic azimuths.
    pub side: Vec3,
    /// Azimuth in radians of the first leaf of node 0.
    pub phase: f32,
    /// Number of metamers built by this meristem.
    pub nodes: u32,
    /// Number of shoots (cycles with at least one metamer) built by this meristem.
    pub shoots: u16,
    /// Number of forks between the first axis of the apparent axis and this one.
    pub forks: u8,
    /// Arc length of the apparent axis at which this chain starts.
    pub start: f32,
    /// Arc length of the apparent axis at the tip.
    pub length: f32,
    /// Length of the full-grown apparent axis, the denominator of relative positions.
    pub target: f32,
    /// Relative position of the attachment of the apparent axis along its parent.
    pub attach_position: f32,
}

impl Meristem {
    /// Random numbers of this meristem for `stream`, starting at draw `counter`.
    #[inline]
    #[must_use]
    pub const fn rng(&self, stream: Stream, counter: u64) -> Rng {
        Rng::at(self.key, 0, stream, counter)
    }

    /// Relative position of the tip along the full-grown axis, in `[0, 1]`.
    #[inline]
    #[must_use]
    pub fn position(&self) -> f32 {
        if self.target > 0.0 {
            (self.length / self.target).min(1.0)
        } else {
            0.0
        }
    }
}

/// Key of stem number `stem` of a plant grown with `seed`.
#[must_use]
pub const fn seed_key(seed: u64, stem: u32) -> u64 {
    mix64(seed ^ mix64(stem as u64 + 1))
}

/// Key of the meristem of bud `slot` of kind `kind` on metamer `node_index` of the
/// meristem with key `parent`.
#[must_use]
pub const fn child_key(parent: u64, kind: SiteKind, node_index: u32, slot: u32) -> u64 {
    let site = ((node_index as u64) << 32) | slot as u64;
    mix64(mix64(parent ^ site).wrapping_add(kind as u64))
}

/// Frame of a node, from which the directions of its leaves and buds derive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NodeFrame {
    /// Unit direction of the axis at the node.
    pub(crate) heading: Vec3,
    /// Unit reference of azimuths, orthogonal to `heading`.
    pub(crate) side: Vec3,
    /// Azimuth in radians of the first leaf of the node.
    pub(crate) azimuth: f32,
    /// Whether the axis keeps its leaves and buds in one plane.
    pub(crate) plagiotropic: bool,
}

/// A node whose leaf sites wait for the end of growth, when retention is known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LeafyNode {
    /// The node, with the id it had when it was created.
    pub(crate) node: NodeId,
    /// Meristem that built the node.
    pub(crate) meristem: MeristemId,
    /// Index of the shoot of the node among the shoots of the meristem.
    pub(crate) shoot: u16,
    /// Growth cycle of the node.
    pub(crate) birth: u16,
    /// Index of the node among the metamers of its meristem.
    pub(crate) index: u32,
    /// Relative position of the node along its full-grown axis.
    pub(crate) position: f32,
    /// Frame of the node.
    pub(crate) frame: NodeFrame,
}

/// A plant under construction.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plant {
    /// The plant graph.
    pub graph: PlantGraph,
    /// Every meristem ever formed, indexed by [`MeristemId`].
    pub meristems: Vec<Meristem>,
    /// Meristem of each chain, indexed by chain id.
    pub(crate) chain_meristems: Vec<MeristemId>,
    /// Every node built so far, with what its leaf sites derive from.
    pub(crate) leafy: Vec<LeafyNode>,
}

impl Plant {
    /// Empty plant.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The meristem `id`.
    #[inline]
    #[must_use]
    pub fn meristem(&self, id: MeristemId) -> &Meristem {
        &self.meristems[id.index()]
    }

    /// Id the next meristem will get.
    #[inline]
    #[must_use]
    pub fn next_meristem(&self) -> MeristemId {
        MeristemId(self.meristems.len() as u32)
    }

    /// Meristem that builds `chain`.
    #[inline]
    #[must_use]
    pub fn meristem_of(&self, chain: ChainId) -> MeristemId {
        self.chain_meristems[chain.index()]
    }
}
