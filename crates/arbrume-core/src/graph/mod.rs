//! The plant graph: an arena of metamers, buds, axes and lateral organ sites.
//!
//! # Model
//!
//! The vocabulary follows Barthelemy and Caraglio (2007). A plant is built by repetition
//! of **metamers**: one internode, the node at its distal end, and the leaves and
//! axillary buds that node bears. Metamers produced by one meristem form an **axis**,
//! called a **chain** here. The trunk has branching order 0; a lateral chain has the
//! order of its parent plus one (Gravelius ordering). A relay chain of a sympodial axis
//! keeps the order of the chain it continues.
//!
//! * A **node** ([`NodeId`]) is one metamer. It stores the position of its distal end;
//!   its proximal end is the position of its parent node, or the origin of its chain for
//!   the first metamer of a root chain. The radius is that of the internode. The length
//!   is derived from the two end points.
//! * A **chain** ([`ChainId`], [`Chain`]) lists the metamers of one axis from base to
//!   tip. Its first node is attached to a node of the parent chain (`attach`), which is
//!   where the axis starts geometrically.
//! * A **bud** ([`BudId`]) is a meristem that may grow: the terminal bud at the tip of a
//!   chain, or a lateral bud on a node. It carries the per-bud state of the growth cycle
//!   of Palubicki et al. (2009): exposure `Q`, preferred direction `V`, resource `v`.
//! * An **organ site** ([`OrganSite`]) is a place where the mesher instances a leaf, a
//!   twig, a flower or a fruit.
//!
//! # Storage and identifiers
//!
//! Nodes and buds are structure-of-arrays: one `Vec` per field, indexed by a `u32` id,
//! with no allocation per element. Geometry is `f32`. Coordinates are meters,
//! right-handed, Y up.
//!
//! **A parent node is always stored before its children**, and a parent chain before its
//! child chains. Iterating node ids upward is therefore an acropetal (base to tip)
//! traversal and iterating downward a basipetal one, with no stack and no child lists;
//! [`PlantGraph::accumulate_basipetal`] and [`PlantGraph::propagate_acropetal`] are built
//! on this.
//!
//! # Chain contiguity
//!
//! The mesher wants each chain as one contiguous slice. Growth only ever appends nodes
//! at the end of the arena, in constant time, so:
//!
//! * starting a chain keeps the layout contiguous;
//! * extending the chain that owns the last node of the arena keeps it contiguous (a
//!   generator that finishes one axis before starting the next never breaks it);
//! * extending any other chain clears [`PlantGraph::is_contiguous`].
//!
//! [`PlantGraph::compact`] restores the guarantee in one linear pass by reordering the
//! arena into depth-first order (see [`PlantGraph::depth_first_order`]). It changes node
//! ids and returns the mapping; chain ids, bud ids and organ indices never change.
//!
//! # Time
//!
//! Every node and bud records the growth cycle in which it appeared (`birth`). Births
//! never decrease from parent to child, so the state of the plant at an earlier cycle is
//! a prefix of every chain ([`ChainView::born_by`]).
//!
//! # Shedding
//!
//! Shed branches stay in the arena with [`NodeFlags::SHED`] set, so that radii keep the
//! memory of past branches. A shed node has only shed descendants; within a chain the
//! shed nodes are a suffix, which [`PlantGraph::chain_view`] leaves out.

mod ids;
mod traverse;
mod validate;
mod view;

pub use ids::{BudId, ChainId, NodeId};
pub use traverse::Children;
pub use validate::GraphError;
pub use view::{Aabb, ChainView};

use glam::Vec3;

/// Bit set of per-node markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NodeFlags(pub u8);

impl NodeFlags {
    /// No flag set.
    pub const NONE: Self = Self(0);
    /// The metamer was shed. Always set on every descendant too.
    pub const SHED: Self = Self(1);
    /// First metamer of a growth unit (the shoot extended in one flush).
    pub const GROWTH_UNIT_START: Self = Self(1 << 1);

    /// `true` when every flag of `other` is set.
    #[inline]
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Union of both flag sets.
    #[inline]
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// `true` when [`NodeFlags::SHED`] is set.
    #[inline]
    #[must_use]
    pub const fn is_shed(self) -> bool {
        self.contains(Self::SHED)
    }
}

/// Data of a new metamer, passed to the functions that append nodes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metamer {
    /// Position of the distal end (the node), in meters.
    pub position: Vec3,
    /// Radius of the internode, in meters.
    pub radius: f32,
    /// Growth cycle in which the metamer appears. Must not be earlier than its parent's.
    pub birth: u16,
    /// Initial flags.
    pub flags: NodeFlags,
}

impl Metamer {
    /// Metamer ending at `position` with the given radius and birth cycle, no flags.
    #[must_use]
    pub const fn new(position: Vec3, radius: f32, birth: u16) -> Self {
        Self {
            position,
            radius,
            birth,
            flags: NodeFlags::NONE,
        }
    }
}

/// How a new chain relates to the chain it is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChainKind {
    /// A lateral axis: branching order of the parent plus one.
    Lateral,
    /// A relay axis of a sympodial branching system: same order as the parent.
    Relay,
}

/// One axis: the metamers produced by one meristem, base to tip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chain {
    /// First (most proximal) node.
    pub first: NodeId,
    /// Last (most distal) node.
    pub last: NodeId,
    /// Number of nodes, at least 1.
    pub len: u32,
    /// Node of the parent chain this chain grows from; [`NodeId::NONE`] for a root chain.
    pub attach: NodeId,
    /// Branching order, 0 for the trunk.
    pub order: u8,
    /// Base point of a root chain. Unused for attached chains, whose base is the position
    /// of `attach`; [`PlantGraph::chain_origin`] returns the right one.
    pub origin: Vec3,
    /// Terminal bud, or [`BudId::NONE`] when the chain has none.
    pub terminal_bud: BudId,
}

/// Position of a bud on its axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BudKind {
    /// At the tip of a chain; extends that chain.
    Terminal,
    /// In a leaf axil on a node; starts a new chain.
    Lateral,
}

/// Activity of a bud.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BudState {
    /// Resting; may become active in a later cycle.
    Dormant,
    /// Able to grow in the current cycle.
    Active,
    /// Aborted, shed or consumed; never grows again.
    Dead,
}

/// Copy of the fields of one bud.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bud {
    /// Node bearing the bud. A terminal bud sits on the last node of its chain.
    pub node: NodeId,
    /// Terminal or lateral.
    pub kind: BudKind,
    /// Activity.
    pub state: BudState,
    /// Unit direction in which the bud would grow.
    pub direction: Vec3,
    /// Direction preferred by the environment (`V`); zero when there is none.
    pub preferred: Vec3,
    /// Exposure `Q` given by the environment.
    pub exposure: f32,
    /// Resource `v` allocated by the bud fate stage.
    pub resource: f32,
    /// Growth cycle in which the bud appeared.
    pub birth: u16,
}

/// Mutable access to the bud fields a growth stage writes, next to the read-only data it
/// needs. All slices are indexed by bud id except `node_positions`.
#[derive(Debug)]
pub struct BudsMut<'a> {
    /// Node bearing each bud.
    pub nodes: &'a [NodeId],
    /// Kind of each bud.
    pub kinds: &'a [BudKind],
    /// Birth cycle of each bud.
    pub births: &'a [u16],
    /// Positions of all nodes, indexed by node id (a bud sits at `node_positions[node]`).
    pub node_positions: &'a [Vec3],
    /// State of each bud.
    pub states: &'a mut [BudState],
    /// Growth direction of each bud.
    pub directions: &'a mut [Vec3],
    /// Preferred direction `V` of each bud.
    pub preferred: &'a mut [Vec3],
    /// Exposure `Q` of each bud.
    pub exposure: &'a mut [f32],
    /// Resource `v` of each bud.
    pub resource: &'a mut [f32],
}

/// Kind of lateral organ instanced at an [`OrganSite`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrganKind {
    /// A single leaf or needle.
    Leaf,
    /// A leafy twig template.
    Twig,
    /// A flower or inflorescence.
    Flower,
    /// A fruit or cone.
    Fruit,
}

/// A place where the mesher instances a lateral organ.
///
/// The organ is anchored on the axis at the position of `node`; the mesher moves it out
/// to the bark surface. `direction` and `normal` are unit and orthogonal; with their
/// cross product they form the frame of the instance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrganSite {
    /// Node bearing the organ.
    pub node: NodeId,
    /// Kind of organ.
    pub kind: OrganKind,
    /// Unit direction of the organ's main axis (for a leaf, base to tip).
    pub direction: Vec3,
    /// Unit normal of the organ (for a leaf, the upper side of the blade).
    pub normal: Vec3,
    /// Size factor applied to the species organ size.
    pub scale: f32,
    /// Growth cycle in which the organ appeared.
    pub birth: u16,
}

/// The plant graph. See the [module documentation](self).
#[derive(Debug, Clone, PartialEq)]
pub struct PlantGraph {
    parent: Vec<NodeId>,
    position: Vec<Vec3>,
    radius: Vec<f32>,
    node_chain: Vec<ChainId>,
    order: Vec<u8>,
    birth: Vec<u16>,
    flags: Vec<NodeFlags>,
    shed_cycle: Vec<u16>,
    chains: Vec<Chain>,
    bud_node: Vec<NodeId>,
    bud_kind: Vec<BudKind>,
    bud_state: Vec<BudState>,
    bud_direction: Vec<Vec3>,
    bud_preferred: Vec<Vec3>,
    bud_exposure: Vec<f32>,
    bud_resource: Vec<f32>,
    bud_birth: Vec<u16>,
    organs: Vec<OrganSite>,
    contiguous: bool,
}

impl Default for PlantGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Construction and growth.
impl PlantGraph {
    /// Empty graph.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            parent: Vec::new(),
            position: Vec::new(),
            radius: Vec::new(),
            node_chain: Vec::new(),
            order: Vec::new(),
            birth: Vec::new(),
            flags: Vec::new(),
            shed_cycle: Vec::new(),
            chains: Vec::new(),
            bud_node: Vec::new(),
            bud_kind: Vec::new(),
            bud_state: Vec::new(),
            bud_direction: Vec::new(),
            bud_preferred: Vec::new(),
            bud_exposure: Vec::new(),
            bud_resource: Vec::new(),
            bud_birth: Vec::new(),
            organs: Vec::new(),
            contiguous: true,
        }
    }

    /// Empty graph with room for `nodes` nodes, `chains` chains and `buds` buds.
    #[must_use]
    pub fn with_capacity(nodes: usize, chains: usize, buds: usize) -> Self {
        let mut graph = Self::new();
        graph.reserve(nodes, chains, buds);
        graph
    }

    /// Reserves room for `nodes` more nodes, `chains` more chains and `buds` more buds.
    pub fn reserve(&mut self, nodes: usize, chains: usize, buds: usize) {
        self.parent.reserve(nodes);
        self.position.reserve(nodes);
        self.radius.reserve(nodes);
        self.node_chain.reserve(nodes);
        self.order.reserve(nodes);
        self.birth.reserve(nodes);
        self.flags.reserve(nodes);
        self.shed_cycle.reserve(nodes);
        self.chains.reserve(chains);
        self.bud_node.reserve(buds);
        self.bud_kind.reserve(buds);
        self.bud_state.reserve(buds);
        self.bud_direction.reserve(buds);
        self.bud_preferred.reserve(buds);
        self.bud_exposure.reserve(buds);
        self.bud_resource.reserve(buds);
        self.bud_birth.reserve(buds);
    }

    fn push_node(&mut self, parent: NodeId, chain: ChainId, order: u8, metamer: Metamer) -> NodeId {
        debug_assert!(parent.is_none() || self.birth[parent.index()] <= metamer.birth);
        let id = NodeId::new(self.parent.len());
        self.parent.push(parent);
        self.position.push(metamer.position);
        self.radius.push(metamer.radius);
        self.node_chain.push(chain);
        self.order.push(order);
        self.birth.push(metamer.birth);
        self.flags.push(metamer.flags);
        self.shed_cycle.push(0);
        id
    }

    fn push_chain(&mut self, attach: NodeId, order: u8, origin: Vec3, metamer: Metamer) -> NodeId {
        let chain = ChainId::new(self.chains.len());
        let node = self.push_node(attach, chain, order, metamer);
        let terminal_bud = BudId::NONE;
        self.chains.push(Chain {
            first: node,
            last: node,
            len: 1,
            attach,
            order,
            origin,
            terminal_bud,
        });
        node
    }

    /// Starts a root chain (order 0) at `origin` with one metamer and returns its node.
    /// The chain id is `self.chain_of(node)`. A plant may have several root chains.
    pub fn add_root_chain(&mut self, origin: Vec3, metamer: Metamer) -> NodeId {
        self.push_chain(NodeId::NONE, 0, origin, metamer)
    }

    /// Starts a chain attached to node `attach` with one metamer and returns its node.
    ///
    /// The internode of the new metamer runs from the position of `attach` to
    /// `metamer.position`. This never breaks contiguity.
    pub fn add_chain(&mut self, attach: NodeId, kind: ChainKind, metamer: Metamer) -> NodeId {
        let parent_order = self.order[attach.index()];
        let order = match kind {
            ChainKind::Lateral => parent_order.saturating_add(1),
            ChainKind::Relay => parent_order,
        };
        self.push_chain(attach, order, Vec3::ZERO, metamer)
    }

    /// Appends one metamer at the tip of `chain` and returns its node. Constant time.
    ///
    /// The terminal bud of the chain, if any, moves to the new node. The layout stays
    /// contiguous only when the previous tip was the last node of the arena.
    pub fn extend_chain(&mut self, chain: ChainId, metamer: Metamer) -> NodeId {
        let Chain {
            last,
            order,
            terminal_bud,
            ..
        } = self.chains[chain.index()];
        self.contiguous &= last.index() + 1 == self.parent.len();
        let node = self.push_node(last, chain, order, metamer);
        let entry = &mut self.chains[chain.index()];
        entry.last = node;
        entry.len += 1;
        if terminal_bud.is_some() {
            self.bud_node[terminal_bud.index()] = node;
        }
        node
    }

    /// Adds a dormant bud on `node`, growing towards the unit vector `direction`.
    ///
    /// A terminal bud must be added on the last node of a chain that has no terminal bud
    /// yet; it becomes the terminal bud of that chain. Exposure and resource start at 0,
    /// the preferred direction at zero, and the birth cycle is that of `node`.
    pub fn add_bud(&mut self, node: NodeId, kind: BudKind, direction: Vec3) -> BudId {
        let bud = BudId::new(self.bud_node.len());
        if kind == BudKind::Terminal {
            let chain = &mut self.chains[self.node_chain[node.index()].index()];
            debug_assert!(chain.last == node && chain.terminal_bud.is_none());
            chain.terminal_bud = bud;
        }
        self.bud_node.push(node);
        self.bud_kind.push(kind);
        self.bud_state.push(BudState::Dormant);
        self.bud_direction.push(direction);
        self.bud_preferred.push(Vec3::ZERO);
        self.bud_exposure.push(0.0);
        self.bud_resource.push(0.0);
        self.bud_birth.push(self.birth[node.index()]);
        bud
    }

    /// Grows a lateral bud into a new chain with one metamer and returns its node.
    ///
    /// The chain is attached to the node bearing the bud. The bud is not consumed: it
    /// becomes the active terminal bud of the new chain, so bud ids stay valid.
    pub fn sprout(&mut self, bud: BudId, kind: ChainKind, metamer: Metamer) -> NodeId {
        debug_assert_eq!(self.bud_kind[bud.index()], BudKind::Lateral);
        let node = self.add_chain(self.bud_node[bud.index()], kind, metamer);
        self.bud_node[bud.index()] = node;
        self.bud_kind[bud.index()] = BudKind::Terminal;
        self.bud_state[bud.index()] = BudState::Active;
        self.chains[self.node_chain[node.index()].index()].terminal_bud = bud;
        node
    }

    /// Marks `node` and all its descendants as shed and kills the buds they bear.
    /// Linear in the number of nodes stored after `node`. The shedding cycle recorded is
    /// 0: the branch counts as absent at every earlier age of the plant.
    pub fn shed_subtree(&mut self, node: NodeId) {
        self.shed_subtrees(&[node], 0);
    }

    /// Marks every node of `roots` and all their descendants as shed during growth cycle
    /// `cycle`, and kills the buds they bear. One linear pass whatever the number of
    /// roots. Nodes that are already shed keep the cycle recorded for them.
    ///
    /// The cycle is what [`PlantGraph::shed_cycles`] returns: the branch was part of the
    /// plant as it stood at the end of every cycle before `cycle`.
    pub fn shed_subtrees(&mut self, roots: &[NodeId], cycle: u16) {
        let Some(start) = roots.iter().map(|node| node.index()).min() else {
            return;
        };
        for root in roots {
            self.mark_shed(root.index(), cycle);
        }
        for id in start + 1..self.parent.len() {
            let parent = self.parent[id];
            if parent.is_some() && self.flags[parent.index()].is_shed() {
                self.mark_shed(id, cycle);
            }
        }
        for (bud, state) in self.bud_state.iter_mut().enumerate() {
            if self.flags[self.bud_node[bud].index()].is_shed() {
                *state = BudState::Dead;
            }
        }
    }

    /// Flags node `id` as shed in `cycle` unless it already is.
    fn mark_shed(&mut self, id: usize, cycle: u16) {
        if !self.flags[id].is_shed() {
            self.flags[id] = self.flags[id].with(NodeFlags::SHED);
            self.shed_cycle[id] = cycle;
        }
    }

    /// Clears the shed flag of the nodes shed after growth cycle `cycle`, which gives
    /// back the branches the plant still bore at the end of that cycle. Buds stay dead.
    pub fn restore_shed_after(&mut self, cycle: u16) {
        for (flags, &shed) in self.flags.iter_mut().zip(&self.shed_cycle) {
            if flags.is_shed() && shed > cycle {
                *flags = NodeFlags(flags.0 & !NodeFlags::SHED.0);
            }
        }
    }

    /// Adds an organ site and returns its index in [`PlantGraph::organs`].
    pub fn add_organ(&mut self, site: OrganSite) -> usize {
        self.organs.push(site);
        self.organs.len() - 1
    }

    /// Removes all organ sites, keeping the allocation.
    pub fn clear_organs(&mut self) {
        self.organs.clear();
    }
}

/// Node access.
impl PlantGraph {
    /// Number of nodes (metamers).
    #[must_use]
    pub const fn node_count(&self) -> usize {
        self.parent.len()
    }

    /// All node ids in storage order, which is an acropetal order.
    pub fn node_ids(&self) -> impl DoubleEndedIterator<Item = NodeId> + ExactSizeIterator + use<> {
        (0..self.parent.len()).map(NodeId::new)
    }

    /// Parent of each node; [`NodeId::NONE`] for the first node of a root chain.
    #[must_use]
    pub fn parents(&self) -> &[NodeId] {
        &self.parent
    }

    /// Distal position of each node.
    #[must_use]
    pub fn positions(&self) -> &[Vec3] {
        &self.position
    }

    /// Distal positions, writable (bending, rescaling).
    #[must_use]
    pub fn positions_mut(&mut self) -> &mut [Vec3] {
        &mut self.position
    }

    /// Internode radius of each node.
    #[must_use]
    pub fn radii(&self) -> &[f32] {
        &self.radius
    }

    /// Internode radii, writable.
    #[must_use]
    pub fn radii_mut(&mut self) -> &mut [f32] {
        &mut self.radius
    }

    /// Chain of each node.
    #[must_use]
    pub fn node_chains(&self) -> &[ChainId] {
        &self.node_chain
    }

    /// Branching order of each node (that of its chain).
    #[must_use]
    pub fn orders(&self) -> &[u8] {
        &self.order
    }

    /// Birth cycle of each node.
    #[must_use]
    pub fn births(&self) -> &[u16] {
        &self.birth
    }

    /// Flags of each node.
    #[must_use]
    pub fn flags(&self) -> &[NodeFlags] {
        &self.flags
    }

    /// Flags, writable. Set [`NodeFlags::SHED`] through [`PlantGraph::shed_subtree`],
    /// which keeps descendants and buds consistent.
    #[must_use]
    pub fn flags_mut(&mut self) -> &mut [NodeFlags] {
        &mut self.flags
    }

    /// Growth cycle in which each shed node was shed; meaningful only for nodes flagged
    /// [`NodeFlags::SHED`].
    #[inline]
    #[must_use]
    pub fn shed_cycles(&self) -> &[u16] {
        &self.shed_cycle
    }

    /// Parent of `node`, `None` for the first node of a root chain.
    #[inline]
    #[must_use]
    pub fn parent(&self, node: NodeId) -> Option<NodeId> {
        let parent = self.parent[node.index()];
        parent.is_some().then_some(parent)
    }

    /// Distal position of `node`.
    #[inline]
    #[must_use]
    pub fn position(&self, node: NodeId) -> Vec3 {
        self.position[node.index()]
    }

    /// Proximal end of the internode of `node`: the position of its parent, or the origin
    /// of its chain when it has none.
    #[inline]
    #[must_use]
    pub fn proximal(&self, node: NodeId) -> Vec3 {
        let parent = self.parent[node.index()];
        if parent.is_some() {
            self.position[parent.index()]
        } else {
            self.chains[self.node_chain[node.index()].index()].origin
        }
    }

    /// Length of the internode of `node`.
    #[inline]
    #[must_use]
    pub fn internode_length(&self, node: NodeId) -> f32 {
        self.position[node.index()].distance(self.proximal(node))
    }

    /// Chain of `node`.
    #[inline]
    #[must_use]
    pub fn chain_of(&self, node: NodeId) -> ChainId {
        self.node_chain[node.index()]
    }

    /// `true` when `node` was shed.
    #[inline]
    #[must_use]
    pub fn is_shed(&self, node: NodeId) -> bool {
        self.flags[node.index()].is_shed()
    }
}

/// Chain access.
impl PlantGraph {
    /// Number of chains (axes).
    #[must_use]
    pub const fn chain_count(&self) -> usize {
        self.chains.len()
    }

    /// All chain ids; a parent chain always comes before its child chains.
    pub fn chain_ids(
        &self,
    ) -> impl DoubleEndedIterator<Item = ChainId> + ExactSizeIterator + use<> {
        (0..self.chains.len()).map(ChainId::new)
    }

    /// All chains, indexed by chain id.
    #[must_use]
    pub fn chains(&self) -> &[Chain] {
        &self.chains
    }

    /// The chain `chain`.
    #[inline]
    #[must_use]
    pub fn chain(&self, chain: ChainId) -> &Chain {
        &self.chains[chain.index()]
    }

    /// Point where `chain` starts: the position of its attachment node, or its stored
    /// origin for a root chain.
    #[inline]
    #[must_use]
    pub fn chain_origin(&self, chain: ChainId) -> Vec3 {
        self.proximal(self.chains[chain.index()].first)
    }

    /// Chain that `chain` is attached to, `None` for a root chain.
    #[inline]
    #[must_use]
    pub fn parent_chain(&self, chain: ChainId) -> Option<ChainId> {
        let attach = self.chains[chain.index()].attach;
        attach.is_some().then(|| self.node_chain[attach.index()])
    }

    /// Nodes of `chain` from tip to base. Works in any layout.
    pub fn chain_nodes_rev(&self, chain: ChainId) -> impl ExactSizeIterator<Item = NodeId> + '_ {
        let Chain { last, len, .. } = self.chains[chain.index()];
        let mut node = last;
        (0..len).map(move |_| {
            let current = node;
            node = self.parent[current.index()];
            current
        })
    }

    /// Arc length of `chain` from its origin to its tip, shed nodes included.
    #[must_use]
    pub fn chain_length(&self, chain: ChainId) -> f32 {
        self.chain_nodes_rev(chain)
            .map(|node| self.internode_length(node))
            .sum()
    }

    /// `true` when every chain occupies a contiguous range of node ids. See the
    /// [module documentation](self).
    #[must_use]
    pub const fn is_contiguous(&self) -> bool {
        self.contiguous
    }

    /// Node indices of `chain`, base to tip, as a range into the node slices. `None`
    /// when the layout is not contiguous.
    #[inline]
    #[must_use]
    pub fn chain_range(&self, chain: ChainId) -> Option<std::ops::Range<usize>> {
        let Chain { first, len, .. } = self.chains[chain.index()];
        self.contiguous
            .then(|| first.index()..first.index() + len as usize)
    }
}

/// Bud and organ access.
impl PlantGraph {
    /// Number of buds, dead ones included.
    #[must_use]
    pub const fn bud_count(&self) -> usize {
        self.bud_node.len()
    }

    /// All bud ids.
    pub fn bud_ids(&self) -> impl DoubleEndedIterator<Item = BudId> + ExactSizeIterator + use<> {
        (0..self.bud_node.len()).map(BudId::new)
    }

    /// Copy of the fields of `bud`.
    #[must_use]
    pub fn bud(&self, bud: BudId) -> Bud {
        let i = bud.index();
        Bud {
            node: self.bud_node[i],
            kind: self.bud_kind[i],
            state: self.bud_state[i],
            direction: self.bud_direction[i],
            preferred: self.bud_preferred[i],
            exposure: self.bud_exposure[i],
            resource: self.bud_resource[i],
            birth: self.bud_birth[i],
        }
    }

    /// Position of `bud`: that of the node bearing it.
    #[inline]
    #[must_use]
    pub fn bud_position(&self, bud: BudId) -> Vec3 {
        self.position[self.bud_node[bud.index()].index()]
    }

    /// Node bearing each bud.
    #[must_use]
    pub fn bud_nodes(&self) -> &[NodeId] {
        &self.bud_node
    }

    /// Kind of each bud.
    #[must_use]
    pub fn bud_kinds(&self) -> &[BudKind] {
        &self.bud_kind
    }

    /// State of each bud.
    #[must_use]
    pub fn bud_states(&self) -> &[BudState] {
        &self.bud_state
    }

    /// Growth direction of each bud.
    #[must_use]
    pub fn bud_directions(&self) -> &[Vec3] {
        &self.bud_direction
    }

    /// Preferred direction `V` of each bud.
    #[must_use]
    pub fn bud_preferred(&self) -> &[Vec3] {
        &self.bud_preferred
    }

    /// Exposure `Q` of each bud.
    #[must_use]
    pub fn bud_exposures(&self) -> &[f32] {
        &self.bud_exposure
    }

    /// Resource `v` of each bud.
    #[must_use]
    pub fn bud_resources(&self) -> &[f32] {
        &self.bud_resource
    }

    /// Sets the state of one bud.
    #[inline]
    pub fn set_bud_state(&mut self, bud: BudId, state: BudState) {
        self.bud_state[bud.index()] = state;
    }

    /// Writable bud fields together with the read-only data needed to compute them, so
    /// that several fields can be filled in one pass (possibly in parallel).
    #[must_use]
    pub fn buds_mut(&mut self) -> BudsMut<'_> {
        BudsMut {
            nodes: &self.bud_node,
            kinds: &self.bud_kind,
            births: &self.bud_birth,
            node_positions: &self.position,
            states: &mut self.bud_state,
            directions: &mut self.bud_direction,
            preferred: &mut self.bud_preferred,
            exposure: &mut self.bud_exposure,
            resource: &mut self.bud_resource,
        }
    }

    /// All organ sites, in insertion order.
    #[must_use]
    pub fn organs(&self) -> &[OrganSite] {
        &self.organs
    }

    /// Organ sites, writable. Keep `node` valid.
    #[must_use]
    pub fn organs_mut(&mut self) -> &mut [OrganSite] {
        &mut self.organs
    }
}

#[cfg(test)]
mod tests {
    use super::{BudKind, BudState, ChainKind, Metamer, NodeFlags, NodeId, OrganKind, OrganSite};
    use super::{PlantGraph, Vec3};

    fn up(height: f32) -> Metamer {
        Metamer::new(Vec3::new(0.0, height, 0.0), 0.1, 0)
    }

    /// Trunk of three nodes with one lateral of two nodes attached to the middle one.
    fn small_tree() -> PlantGraph {
        let mut graph = PlantGraph::new();
        let base = graph.add_root_chain(Vec3::ZERO, up(1.0));
        let trunk = graph.chain_of(base);
        let middle = graph.extend_chain(trunk, up(2.0));
        graph.extend_chain(trunk, up(3.0));
        let side = graph.add_chain(
            middle,
            ChainKind::Lateral,
            Metamer::new(Vec3::new(1.0, 2.0, 0.0), 0.05, 1),
        );
        graph.extend_chain(
            graph.chain_of(side),
            Metamer::new(Vec3::new(2.0, 2.0, 0.0), 0.05, 1),
        );
        graph
    }

    #[test]
    fn building_keeps_the_layout_contiguous_when_chains_are_finished_in_turn() {
        let graph = small_tree();
        graph.check_invariants().unwrap();
        assert!(graph.is_contiguous());
        assert_eq!(graph.node_count(), 5);
        assert_eq!(graph.chain_count(), 2);
        assert_eq!(graph.orders(), &[0, 0, 0, 1, 1]);
        assert_eq!(
            graph.chain_range(graph.chain_of(NodeId::new(3))),
            Some(3..5)
        );
        assert!((graph.chain_length(graph.chain_of(NodeId::new(0))) - 3.0).abs() < 1e-6);
        assert!((graph.internode_length(NodeId::new(3)) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn extending_an_older_chain_breaks_contiguity_until_compaction() {
        let mut graph = small_tree();
        let trunk = graph.chain_of(NodeId::new(0));
        graph.extend_chain(trunk, up(4.0));
        assert!(!graph.is_contiguous());
        assert_eq!(graph.chain_range(trunk), None);
        graph.check_invariants().unwrap();
        let remap = graph.compact().unwrap();
        assert!(graph.is_contiguous());
        graph.check_invariants().unwrap();
        assert_eq!(remap[5], NodeId::new(3));
        assert_eq!(graph.chain_range(trunk), Some(0..4));
        assert_eq!(graph.compact(), None);
    }

    #[test]
    fn buds_follow_growth() {
        let mut graph = small_tree();
        let trunk = graph.chain_of(NodeId::new(0));
        let tip = graph.chain(trunk).last;
        let terminal = graph.add_bud(tip, BudKind::Terminal, Vec3::Y);
        let lateral = graph.add_bud(NodeId::new(0), BudKind::Lateral, Vec3::X);
        let new_tip = graph.extend_chain(trunk, up(4.0));
        assert_eq!(graph.bud(terminal).node, new_tip);
        let shoot = graph.sprout(lateral, ChainKind::Lateral, Metamer::new(Vec3::X, 0.02, 2));
        let bud = graph.bud(lateral);
        assert_eq!(
            (bud.node, bud.kind, bud.state),
            (shoot, BudKind::Terminal, BudState::Active)
        );
        assert_eq!(graph.chain(graph.chain_of(shoot)).terminal_bud, lateral);
        assert_eq!(graph.bud_position(lateral), Vec3::X);
        graph.check_invariants().unwrap();
        graph.compact();
        graph.check_invariants().unwrap();
        assert_eq!(graph.bud_position(lateral), Vec3::X);
    }

    #[test]
    fn relay_chains_keep_the_order() {
        let mut graph = small_tree();
        let relay = graph.add_chain(
            NodeId::new(4),
            ChainKind::Relay,
            Metamer::new(Vec3::ONE, 0.05, 2),
        );
        assert_eq!(graph.orders()[relay.index()], 1);
        graph.check_invariants().unwrap();
    }

    #[test]
    fn shedding_marks_the_subtree_and_kills_its_buds() {
        let mut graph = small_tree();
        let on_side = graph.add_bud(NodeId::new(4), BudKind::Terminal, Vec3::X);
        let on_trunk = graph.add_bud(NodeId::new(0), BudKind::Lateral, Vec3::X);
        graph.shed_subtree(NodeId::new(1));
        let shed: Vec<bool> = graph.flags().iter().map(|f| f.is_shed()).collect();
        assert_eq!(shed, [false, true, true, true, true]);
        assert_eq!(graph.bud(on_side).state, BudState::Dead);
        assert_eq!(graph.bud(on_trunk).state, BudState::Dormant);
        graph.check_invariants().unwrap();
        let view = graph.chain_view(graph.chain_of(NodeId::new(0))).unwrap();
        assert_eq!(view.len(), 1);
        assert!(
            graph
                .chain_view(graph.chain_of(NodeId::new(3)))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn organs_are_remapped_by_compaction() {
        let mut graph = small_tree();
        let trunk = graph.chain_of(NodeId::new(0));
        let tip = graph.extend_chain(trunk, up(4.0));
        let site = OrganSite {
            node: tip,
            kind: OrganKind::Leaf,
            direction: Vec3::X,
            normal: Vec3::Y,
            scale: 1.0,
            birth: 0,
        };
        assert_eq!(graph.add_organ(site), 0);
        graph.compact();
        assert_eq!(
            graph.position(graph.organs()[0].node),
            Vec3::new(0.0, 4.0, 0.0)
        );
        graph.check_invariants().unwrap();
        graph.clear_organs();
        assert_eq!(graph.organs(), []);
    }

    #[test]
    fn flags_combine() {
        let both = NodeFlags::SHED.with(NodeFlags::GROWTH_UNIT_START);
        assert!(both.is_shed() && both.contains(NodeFlags::GROWTH_UNIT_START));
        assert!(!NodeFlags::NONE.is_shed());
    }
}
