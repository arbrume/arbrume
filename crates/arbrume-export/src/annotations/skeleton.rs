//! The skeleton sidecar: nodes and chains in depth-first order, and organ sites.

use arbrume_core::graph::{
    ChainId, ChainKind, Metamer, NodeFlags, NodeId, OrganKind, OrganSite, PlantGraph,
};
use glam::Vec3;

/// The skeleton of a plant as flat arrays, the depth-first serialization of its graph.
///
/// **Nodes** are in the order of `PlantGraph::depth_first_order`: each chain is one
/// contiguous run of rows from base to tip, the subtree of any node is one contiguous
/// run starting at that node, and a parent row always precedes its children. A node is
/// one metamer: `node_position` is its distal end, `node_radius` the radius of its
/// internode, `node_parent` the row of the node its internode starts from (`-1` for the
/// first node of a root chain, which starts at `chain_origin`). This is the node
/// signature of Zhou et al. (2023): position, radius, parent, order and age (`birth`,
/// the growth cycle in which the metamer appeared), with the arc lengths from the root
/// and from the base of the chain.
///
/// **Chains** (axes) are in the order in which the depth-first traversal reaches them.
/// `chain_id` is the chain id of the graph, which is also the instance id of the bark
/// labels; `chain_parent` is the chain id of the parent chain (`-1` for a root chain).
///
/// **Organs** are indexed by organ index, the instance id of the leaf and twig labels.
///
/// Shed nodes are kept with `node_shed = 1`; they are always a suffix of their chain
/// and have only shed descendants.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Skeleton {
    /// Distal end of each node, meters.
    pub node_position: Vec<[f32; 3]>,
    /// Internode radius, meters.
    pub node_radius: Vec<f32>,
    /// Row of the parent node, `-1` for none.
    pub node_parent: Vec<i32>,
    /// Branching order (0 for the trunk).
    pub node_order: Vec<u8>,
    /// Growth cycle in which the node appeared.
    pub node_birth: Vec<u16>,
    /// Chain id of the node.
    pub node_chain: Vec<u32>,
    /// 1 when the node was shed.
    pub node_shed: Vec<u8>,
    /// Arc length from the base of the plant to the distal end, meters.
    pub node_dist_root: Vec<f32>,
    /// Arc length from the base of the chain to the distal end, meters.
    pub node_dist_chain: Vec<f32>,
    /// Chain id of each chain row.
    pub chain_id: Vec<u32>,
    /// Row of the first (most proximal) node.
    pub chain_first: Vec<u32>,
    /// Row of the last (most distal) node.
    pub chain_last: Vec<u32>,
    /// Chain id of the parent chain, `-1` for a root chain.
    pub chain_parent: Vec<i32>,
    /// Row of the node of the parent chain the chain grows from, `-1` for a root chain.
    pub chain_attach: Vec<i32>,
    /// Branching order.
    pub chain_order: Vec<u8>,
    /// Base point of the chain: the position of its attachment node, or the base of a
    /// root chain.
    pub chain_origin: Vec<[f32; 3]>,
    /// Row of the node bearing each organ.
    pub organ_node: Vec<u32>,
    /// Kind: 0 leaf, 1 twig, 2 flower, 3 fruit.
    pub organ_kind: Vec<u8>,
    /// Unit direction of the organ axis.
    pub organ_direction: Vec<[f32; 3]>,
    /// Unit normal of the organ.
    pub organ_normal: Vec<[f32; 3]>,
    /// Size factor on the species organ size.
    pub organ_scale: Vec<f32>,
    /// Growth cycle in which the organ appeared.
    pub organ_birth: Vec<u16>,
}

/// Value written for an [`OrganKind`].
#[must_use]
pub const fn organ_kind_code(kind: OrganKind) -> u8 {
    match kind {
        OrganKind::Leaf => 0,
        OrganKind::Twig => 1,
        OrganKind::Flower => 2,
        OrganKind::Fruit => 3,
    }
}

const fn organ_kind_of(code: u8) -> Option<OrganKind> {
    match code {
        0 => Some(OrganKind::Leaf),
        1 => Some(OrganKind::Twig),
        2 => Some(OrganKind::Flower),
        3 => Some(OrganKind::Fruit),
        _ => None,
    }
}

/// `-1` for an absent id, the index otherwise.
fn signed(index: Option<usize>) -> i32 {
    index.map_or(-1, |index| {
        i32::try_from(index).expect("graph ids are below 2^31")
    })
}

impl Skeleton {
    /// Number of nodes.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.node_position.len()
    }

    /// Serializes `graph`. Linear in its size apart from sorting sibling chains.
    #[must_use]
    pub fn from_graph(graph: &PlantGraph) -> Self {
        let order = graph.depth_first_order();
        let mut row = vec![0_u32; order.len()];
        for (new, old) in order.iter().enumerate() {
            row[old.index()] = new as u32;
        }
        let row_of = |node: NodeId| node.is_some().then(|| row[node.index()] as usize);
        let mut skeleton = Self::default();
        skeleton.nodes(graph, &order, &row_of);
        skeleton.chains(graph, &row_of);
        for site in graph.organs() {
            skeleton.organ_node.push(row[site.node.index()]);
            skeleton.organ_kind.push(organ_kind_code(site.kind));
            skeleton.organ_direction.push(site.direction.to_array());
            skeleton.organ_normal.push(site.normal.to_array());
            skeleton.organ_scale.push(site.scale);
            skeleton.organ_birth.push(site.birth);
        }
        skeleton
    }

    /// Fills the node arrays in depth-first order.
    fn nodes(
        &mut self,
        graph: &PlantGraph,
        order: &[NodeId],
        row_of: &impl Fn(NodeId) -> Option<usize>,
    ) {
        let count = order.len();
        self.node_dist_root = Vec::with_capacity(count);
        self.node_dist_chain = Vec::with_capacity(count);
        for &node in order {
            let i = node.index();
            let parent = graph.parents()[i];
            let length = graph.internode_length(node);
            let parent_row = row_of(parent);
            let (from_root, from_chain) = parent_row.map_or((0.0, 0.0), |p| {
                let same = graph.chain_of(parent) == graph.chain_of(node);
                let chain = if same { self.node_dist_chain[p] } else { 0.0 };
                (self.node_dist_root[p], chain)
            });
            self.node_position.push(graph.positions()[i].to_array());
            self.node_radius.push(graph.radii()[i]);
            self.node_parent.push(signed(parent_row));
            self.node_order.push(graph.orders()[i]);
            self.node_birth.push(graph.births()[i]);
            self.node_chain.push(graph.node_chains()[i].0);
            self.node_shed.push(u8::from(graph.flags()[i].is_shed()));
            self.node_dist_root.push(from_root + length);
            self.node_dist_chain.push(from_chain + length);
        }
    }

    /// Fills the chain arrays in the order the depth-first traversal reaches the chains.
    fn chains(&mut self, graph: &PlantGraph, row_of: &impl Fn(NodeId) -> Option<usize>) {
        let mut chains: Vec<(u32, ChainId)> = graph
            .chain_ids()
            .map(|chain| (row_of(graph.chain(chain).first).unwrap_or(0) as u32, chain))
            .collect();
        chains.sort_unstable();
        for (first, id) in chains {
            let chain = graph.chain(id);
            self.chain_id.push(id.0);
            self.chain_first.push(first);
            self.chain_last.push(row_of(chain.last).unwrap_or(0) as u32);
            let parent = graph.parent_chain(id).map(ChainId::index);
            self.chain_parent.push(signed(parent));
            self.chain_attach.push(signed(row_of(chain.attach)));
            self.chain_order.push(chain.order);
            self.chain_origin.push(graph.chain_origin(id).to_array());
        }
    }

    /// Rebuilds the plant graph: the same nodes, chains with the same ids, the same
    /// organ sites, in depth-first layout. Buds are not part of the skeleton and are
    /// absent. Inverse of [`Skeleton::from_graph`] up to buds and shedding cycles.
    ///
    /// # Errors
    ///
    /// Returns a message when the arrays are inconsistent: lengths, chain ids that are
    /// not a permutation, references out of range, or a parent chain that would come
    /// after its child.
    pub fn to_graph(&self) -> Result<PlantGraph, String> {
        self.check_lengths()?;
        let chains = self.chain_id.len();
        let mut row_of_chain = vec![usize::MAX; chains];
        for (row, &id) in self.chain_id.iter().enumerate() {
            let slot = row_of_chain
                .get_mut(id as usize)
                .ok_or("chain id out of range")?;
            if *slot != usize::MAX {
                return Err("chain id given twice".to_owned());
            }
            *slot = row;
        }
        let mut graph = PlantGraph::with_capacity(self.node_count(), chains, 0);
        let mut node_of_row = vec![NodeId::NONE; self.node_count()];
        for &row in &row_of_chain {
            self.add_chain(row, &mut graph, &mut node_of_row)?;
        }
        for organ in 0..self.organ_node.len() {
            let node = *node_of_row
                .get(self.organ_node[organ] as usize)
                .ok_or("organ node out of range")?;
            graph.add_organ(OrganSite {
                node,
                kind: organ_kind_of(self.organ_kind[organ]).ok_or("unknown organ kind")?,
                direction: Vec3::from_array(self.organ_direction[organ]),
                normal: Vec3::from_array(self.organ_normal[organ]),
                scale: self.organ_scale[organ],
                birth: self.organ_birth[organ],
            });
        }
        graph.compact();
        Ok(graph)
    }

    /// Appends the chain of row `row` to `graph`; its parent chain must be in already.
    fn add_chain(
        &self,
        row: usize,
        graph: &mut PlantGraph,
        node_of_row: &mut [NodeId],
    ) -> Result<(), String> {
        let first = self.chain_first[row] as usize;
        let last = self.chain_last[row] as usize;
        if first > last || last >= self.node_count() {
            return Err(format!("chain row {row}: bad node range"));
        }
        let metamer = |node: usize| Metamer {
            position: Vec3::from_array(self.node_position[node]),
            radius: self.node_radius[node],
            birth: self.node_birth[node],
            flags: if self.node_shed[node] == 1 {
                NodeFlags::SHED
            } else {
                NodeFlags::NONE
            },
        };
        let start = if let Ok(attach) = usize::try_from(self.chain_attach[row]) {
            let attach = *node_of_row
                .get(attach)
                .filter(|node| node.is_some())
                .ok_or_else(|| format!("chain row {row}: parent chain not built yet"))?;
            let kind = if graph.orders()[attach.index()] == self.chain_order[row] {
                ChainKind::Relay
            } else {
                ChainKind::Lateral
            };
            graph.add_chain(attach, kind, metamer(first))
        } else {
            let origin = Vec3::from_array(self.chain_origin[row]);
            graph.add_root_chain(origin, metamer(first))
        };
        if graph.chain_of(start).index() != self.chain_id[row] as usize {
            return Err(format!(
                "chain row {row}: chain ids are not in creation order"
            ));
        }
        node_of_row[first] = start;
        let chain = graph.chain_of(start);
        for (node, slot) in node_of_row[first + 1..=last].iter_mut().enumerate() {
            *slot = graph.extend_chain(chain, metamer(first + 1 + node));
        }
        Ok(())
    }

    fn check_lengths(&self) -> Result<(), String> {
        let nodes = self.node_count();
        let node_arrays = [
            self.node_radius.len(),
            self.node_parent.len(),
            self.node_order.len(),
            self.node_birth.len(),
            self.node_chain.len(),
            self.node_shed.len(),
            self.node_dist_root.len(),
            self.node_dist_chain.len(),
        ];
        let chains = self.chain_id.len();
        let chain_arrays = [
            self.chain_first.len(),
            self.chain_last.len(),
            self.chain_parent.len(),
            self.chain_attach.len(),
            self.chain_order.len(),
            self.chain_origin.len(),
        ];
        let organs = self.organ_node.len();
        let organ_arrays = [
            self.organ_kind.len(),
            self.organ_direction.len(),
            self.organ_normal.len(),
            self.organ_scale.len(),
            self.organ_birth.len(),
        ];
        let same = node_arrays.iter().all(|&n| n == nodes)
            && chain_arrays.iter().all(|&n| n == chains)
            && organ_arrays.iter().all(|&n| n == organs);
        if same {
            Ok(())
        } else {
            Err("skeleton arrays have inconsistent lengths".to_owned())
        }
    }
}
